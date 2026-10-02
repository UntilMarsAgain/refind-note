//! 与 S3 兼容服务同步：把整份仓库搬到云端，或从云端拿回来。
//!
//! ## 传什么、不传什么
//!
//! 传的是**仓库**：`db/**`（内容块、事件日志、那几张表）与 `settings/repository.json`
//! （仓库自己的设置 —— 换台机器读同一份仓库，行为要一样）。
//!
//! 不传的是**这台机器自己的东西**：`settings/preferences.json`（界面偏好）、
//! `settings/browsing.jsonl`（浏览历史）、`db/drafts/**`（写了一半的草稿槽位）、
//! 以及同步自己的设置与索引（里面还有 S3 的密钥，当然不能往上传）。
//!
//! ## 怎么定谁新
//!
//! 每个文件在 `settings/sync-index.json` 里记一份"上次对齐时的样子"（大小、时间、
//! 云端的 ETag）。于是三种情形分得清：
//!
//! - 只有本机变了 → 上传；
//! - 只有云端变了 → 下载；
//! - 两边都变了 → **按修改时间取新的那一版**，并在报告里说一声（不静默地丢东西）。
//!
//! 内容块（`db/blobs/…`）是**内容寻址**的：文件名就是内容的哈希，所以它们只会
//! "这边有那边没有"，不存在冲突 —— 这一条省掉了一大半麻烦。
//!
//! ## 删除
//!
//! 本机删掉的（上次同步时还在、现在没了）会在云端也删掉；反过来，云端没了而本机
//! 没动过的那一份，本机也删。**两边都没动过的，谁也删不动谁** —— 第一原则是别丢东西。
//!
//! ## 什么时候跑
//!
//! **要在打开数据库之前跑**（启动时那一次尤其）：新机器上 `db/titles.json`、
//! `db/namespaces.json` 这些表是打开数据库时**当场建出来的空表** —— 先开库再同步，
//! 空表会被当成"本机改过、而且更新"，把云端那份真的盖掉。所以界面那边是
//! 先同步、再打开工作目录（见 `core/preferences.ts` 里的顺序说明）。
//!
//! ## 同时只让一台机器动
//!
//! 动手之前先在云端抢一把锁（`PutObject` 带 `If-None-Match: *`，这是 S3 原生的
//! 原子操作）。锁里写着机器名与时间；超过 [`LOCK_TTL`] 没续的算过期，别人可以抢 ——
//! 机器崩了不该把同步永久锁死。

use std::cell::Cell;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::storage::s3::{S3Config, S3};
use crate::storage::workspace::{read_json, write_json, Workspace};

/// 锁多久没动静算过期（秒）
const LOCK_TTL: i64 = 300;

/// 干活期间每隔这么久续一次锁。
///
/// [`LOCK_TTL`] 的意思是"**多久没动静**就当它死了"，不是"一趟最多干多久"。
/// 不续的话，一趟超过 5 分钟的大同步会被别的机器当成过期抢走 —— 那正是这把锁要防
/// 的事。所以每 [`LOCK_REFRESH`] 就把时间戳写成现在（见 [`Lock::touch`]）。
const LOCK_REFRESH: Duration = Duration::from_secs(60);

/// 每走这么多步就把索引落一次盘。
///
/// 索引是"哪些文件已经对齐"的记账本：一直攒到最后才写的话，中途被打断
/// （关窗前那颗"不等了"、断电、进程被杀）就得**从头再对一遍** —— 东西不会坏，
/// 但白传的那些要重传。隔一段写一次，中断最多让你重做这一段的活。
const INDEX_SAVE_EVERY: usize = 25;

/// 锁放在云端哪个键上（在配置的前缀之下）
const LOCK_KEY: &str = ".sync-lock.json";

/// 云端那份**账本**：上一次同步跑完时的索引副本（在配置的前缀之下）。
///
/// 本地那份记的是"这台机器上次对齐时什么样"，它答不了两件事：
///
/// 1. **这台机器是新装的**（本地没有账）—— 那就照云端这份认路，别把云端已有的每一份
///    都当成"本机新写的"再传一遍（"新仓库盖掉云端"那个老毛病，根子也在这儿）；
/// 2. **清单里没有某一份**时，它到底是"被别的机器删了"，还是"桶被清空/清单没列全"。
///    本地账答不了，云端账能：两处都说没有，才是真删了。
///
/// 判"删没删"这件事上，**宁可多传一份，也别删人东西** —— 这一份就是那条底线的凭据。
const LATEST_KEY: &str = ".sync-latest.json";

/// 上传前后给字节过一道的钩子。
///
/// 现在只有"原样"这一种实现，但它把**位置**先留出来：将来要让云端那一份也加密，
/// 实现一个 [`SyncTransform`] 传进 [`run`] 就行 —— 这个模块不必知道自己运的是什么，
/// 也就不会因为"加密怎么做的"而改动（同一份独立性，与"路径该不该同步"交给工作目录
/// 那一层回答是一个道理）。
pub trait SyncTransform: Send + Sync {
    /// 上传之前：本机的字节 → 传上去的字节
    fn seal(&self, relative: &str, bytes: Vec<u8>) -> Result<Vec<u8>, String>;
    /// 下载之后：云端的字节 → 本机的字节
    fn open(&self, relative: &str, bytes: Vec<u8>) -> Result<Vec<u8>, String>;
}

/// 原样（不加密）——默认就是这个
pub struct Plaintext;

impl SyncTransform for Plaintext {
    fn seal(&self, _relative: &str, bytes: Vec<u8>) -> Result<Vec<u8>, String> {
        Ok(bytes)
    }
    fn open(&self, _relative: &str, bytes: Vec<u8>) -> Result<Vec<u8>, String> {
        Ok(bytes)
    }
}

/// 同步自己的设置（**只在这台机器上**：里面是密钥）
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SyncSettings {
    /// 启动时自动同步一次
    pub enabled: bool,
    /// 传上去之前要不要再套一层（钥匙就是下面这一把）
    pub encrypt: bool,
    /// 这一层用哪一档对称加密（与笔记那边同一个枚举）。
    ///
    /// 与钥匙分开：**钥匙不必跟着换**。SM4 的密钥是 128 位，从这把 32 字节的钥匙里
    /// 取前 16 字节（见 `CloudCipher`），所以换算法只用改这一栏。
    /// 已经传上去的东西也不受影响 —— 每一份封装的头里记着自己那一档（见 `CloudEnvelope`），
    /// 解得开旧的就还是解得开，只有**往后新传的**才用新选的。
    pub cipher: crate::storage::codec::Cipher,
    /// 下一次同步**把本机这份整份重传**（换了钥匙就得这样：云端那些旧密文已经解不开了）。
    /// 跑完这一趟就自动清掉。
    #[serde(default)]
    pub reupload: bool,
    /// **软件生成的**那把钥匙（base64，32 字节）。
    ///
    /// 它只在这台机器上（`settings/sync.json`，权限只给本人）——换台机器要把这串抄过去，
    /// 抄不过去，云端那一份就解不开了。丢了也没有后路：那是这把钥匙的意义所在。
    pub key: String,
    pub s3: S3Config,
}

impl SyncSettings {
    /// 能同步吗：开着、而且该填的都填了
    pub fn is_ready(&self) -> bool {
        self.enabled && self.s3.is_usable()
    }

    /// 这一份设置里那把钥匙 + 选的那一档算法（认得出才给）
    pub fn cipher(&self) -> Result<CloudCipher, String> {
        CloudCipher::from_key(&self.key).map(|cipher| cipher.with_cipher(self.cipher))
    }
}

/// 给**界面**看的那一份：**不含任何秘密**。
///
/// 密钥与 S3 的私钥都不出去 —— 存本地至少得碰到这台电脑，显示出来就不一定了
/// （直播、共享屏幕、随手截个图，都可能把它带出去）。界面只需要知道"配没配"。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SyncSettingsView {
    pub enabled: bool,
    pub encrypt: bool,
    /// 云端那一层用哪一档（不是秘密，界面要能显示与修改）
    pub cipher: crate::storage::codec::Cipher,
    /// 云端密钥配好了没有（**钥匙本身不出去**）
    pub has_key: bool,
    /// S3 私钥配好了没有
    pub has_secret: bool,
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    pub prefix: String,
    pub access_key: String,
}

impl SyncSettings {
    /// 交出去的那一份（秘密只报"有没有"）
    pub fn view(&self) -> SyncSettingsView {
        SyncSettingsView {
            enabled: self.enabled,
            encrypt: self.encrypt,
            cipher: self.cipher,
            has_key: !self.key.is_empty(),
            has_secret: !self.s3.secret_key.is_empty(),
            endpoint: self.s3.endpoint.clone(),
            region: self.s3.region.clone(),
            bucket: self.s3.bucket.clone(),
            prefix: self.s3.prefix.clone(),
            access_key: self.s3.access_key.clone(),
        }
    }

    /// 界面改完交回来：秘密**留空就是不改**（界面根本拿不到它们，也就无从交回）
    pub fn apply(&mut self, patch: SyncSettingsPatch) {
        self.enabled = patch.enabled;
        self.encrypt = patch.encrypt;
        self.cipher = patch.cipher;
        self.s3.endpoint = patch.endpoint;
        self.s3.region = patch.region;
        self.s3.bucket = patch.bucket;
        self.s3.prefix = patch.prefix;
        self.s3.access_key = patch.access_key;
        if let Some(secret) = patch.secret_key {
            self.s3.secret_key = secret;
        }
    }
}

/// 界面上改完交回来的那一份
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct SyncSettingsPatch {
    pub enabled: bool,
    pub encrypt: bool,
    pub cipher: crate::storage::codec::Cipher,
    pub endpoint: String,
    pub region: String,
    pub bucket: String,
    pub prefix: String,
    pub access_key: String,
    /// `None` = 不改；`Some("")` = 清掉
    pub secret_key: Option<String>,
}

/// 生成一把新的云端钥匙：32 字节随机，写成 base64
pub fn generate_key() -> Result<String, String> {
    use base64::Engine as _;

    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes).map_err(|error| format!("取随机数失败：{error}"))?;
    Ok(base64::engine::general_purpose::STANDARD.encode(bytes))
}

// ---------------------------------------------------------------- 云端那一层

/// 传上去的每一份都过这一层：**整份仓库**因此一起被保护起来 ——
/// 日志、标题表、命名空间表，以及那些没单独加密的笔记，都在内。
///
/// 与笔记自身那几层（gpg / 口令）是**两件事**：那几层防的是"拿到你这台机器的人"，
/// 这一层防的是"存储服务、以及捡到那个桶的人"。两者不重叠，各管各的。
///
/// 封出来的东西自带说明（见 [`CloudEnvelope`]）：没有这一层的老对象（明文传上去的）
/// 读的时候原样放行 —— 换了设置不至于把已经传上去的东西读废。
pub struct CloudCipher {
    key: [u8; 32],
    cipher: crate::storage::codec::Cipher,
}

/// 云端的封装：`RNDS` + 版本 + 这一档算法 + nonce + 密文。
///
/// 头上写着用哪一档算法，是为了"以后换了算法还读得回旧的"——与 blob 那边同一个道理。
struct CloudEnvelope;

impl CloudEnvelope {
    const MAGIC: &'static [u8; 4] = b"RNDS";
    const VERSION: u8 = 1;
    /// magic(4) + version(1) + cipher(1) + nonce(12)
    const HEADER: usize = 18;

    /// 这一串字节是不是我们封出来的
    fn matches(bytes: &[u8]) -> bool {
        bytes.len() >= Self::HEADER && &bytes[..4] == Self::MAGIC && bytes[4] == Self::VERSION
    }

    fn cipher_byte(cipher: crate::storage::codec::Cipher) -> u8 {
        use crate::storage::codec::Cipher;
        match cipher {
            Cipher::Aes256Gcm => 1,
            Cipher::Sm4Gcm => 2,
        }
    }

    fn cipher_of(byte: u8) -> Option<crate::storage::codec::Cipher> {
        use crate::storage::codec::Cipher;
        match byte {
            1 => Some(Cipher::Aes256Gcm),
            2 => Some(Cipher::Sm4Gcm),
            _ => None,
        }
    }
}

impl CloudCipher {
    /// 从 base64 那把钥匙做出来；空串（没设）返回 `Err`
    pub fn from_key(key: &str) -> Result<Self, String> {
        use base64::Engine as _;
        let raw = base64::engine::general_purpose::STANDARD
            .decode(key.trim())
            .map_err(|_| "云端密钥不是一串 base64，抄的时候可能少了几个字".to_string())?;
        if raw.len() != 32 {
            return Err(format!("云端密钥应该是 32 字节，这把是 {} 字节", raw.len()));
        }
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&raw);
        Ok(Self {
            key: bytes,
            // 一开始按默认那一档；真正用哪一档由调用方说了算
            // （同步那边是 `SyncSettings::cipher` 里的设置，见 `with_cipher`）
            cipher: crate::storage::codec::Cipher::default(),
        })
    }

    /// 换一档算法（同步层与笔记那边用同一档，界面上不单独问）
    pub fn with_cipher(mut self, cipher: crate::storage::codec::Cipher) -> Self {
        self.cipher = cipher;
        self
    }
}

impl SyncTransform for CloudCipher {
    fn seal(&self, relative: &str, bytes: Vec<u8>) -> Result<Vec<u8>, String> {
        // 每个对象一条独立的 nonce：同一份内容传两次，密文也不一样
        let mut nonce = [0u8; 12];
        getrandom::getrandom(&mut nonce).map_err(|error| format!("取随机数失败：{error}"))?;
        let sealed = seal_bytes(self.cipher, &self.key, &nonce, &bytes)
            .map_err(|error| format!("加密 {relative} 失败：{error}"))?;

        let mut out = Vec::with_capacity(CloudEnvelope::HEADER + sealed.len());
        out.extend_from_slice(CloudEnvelope::MAGIC);
        out.push(CloudEnvelope::VERSION);
        out.push(CloudEnvelope::cipher_byte(self.cipher));
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&sealed);
        Ok(out)
    }

    fn open(&self, relative: &str, bytes: Vec<u8>) -> Result<Vec<u8>, String> {
        // 不是我们封的（没开加密时传上去的老对象）：原样放行
        if !CloudEnvelope::matches(&bytes) {
            return Ok(bytes);
        }
        let cipher = CloudEnvelope::cipher_of(bytes[5])
            .ok_or_else(|| format!("{relative} 用了本程序不认识的加密档"))?;
        let nonce = &bytes[6..CloudEnvelope::HEADER];
        open_bytes(cipher, &self.key, nonce, &bytes[CloudEnvelope::HEADER..])
            .map_err(|error| format!("解开 {relative} 失败（密钥对不对？）：{error}"))
    }
}

/// 用这一档算法封上（与笔记那边同一套 AEAD，只是头不同 —— 这里要的是"自带说明"）
fn seal_bytes(
    cipher: crate::storage::codec::Cipher,
    key: &[u8; 32],
    nonce: &[u8],
    content: &[u8],
) -> Result<Vec<u8>, String> {
    use crate::storage::codec::Cipher;
    use aes_gcm::aead::{Aead, KeyInit};

    match cipher {
        Cipher::Aes256Gcm => {
            let cipher = aes_gcm::Aes256Gcm::new_from_slice(key)
                .map_err(|error| format!("密钥长度不对：{error}"))?;
            let nonce =
                aes_gcm::Nonce::try_from(nonce).map_err(|_| "nonce 长度不对".to_string())?;
            cipher
                .encrypt(&nonce, content)
                .map_err(|_| "加密失败".to_string())
        }
        Cipher::Sm4Gcm => {
            let cipher =
                aes_gcm::AesGcm::<sm4::Sm4, aes_gcm::aead::consts::U12>::new_from_slice(&key[..16])
                    .map_err(|error| format!("密钥长度不对：{error}"))?;
            let nonce =
                aes_gcm::Nonce::try_from(nonce).map_err(|_| "nonce 长度不对".to_string())?;
            cipher
                .encrypt(&nonce, content)
                .map_err(|_| "加密失败".to_string())
        }
    }
}

/// 照头里那一档算法解开
fn open_bytes(
    cipher_kind: crate::storage::codec::Cipher,
    key: &[u8; 32],
    nonce: &[u8],
    payload: &[u8],
) -> Result<Vec<u8>, String> {
    use crate::storage::codec::Cipher;
    use aes_gcm::aead::{Aead, KeyInit};

    match cipher_kind {
        Cipher::Aes256Gcm => {
            let cipher = aes_gcm::Aes256Gcm::new_from_slice(key)
                .map_err(|error| format!("密钥长度不对：{error}"))?;
            let nonce =
                aes_gcm::Nonce::try_from(nonce).map_err(|_| "nonce 长度不对".to_string())?;
            cipher
                .decrypt(&nonce, payload)
                .map_err(|_| "口令不对，或者内容被改过".to_string())
        }
        Cipher::Sm4Gcm => {
            let cipher =
                aes_gcm::AesGcm::<sm4::Sm4, aes_gcm::aead::consts::U12>::new_from_slice(&key[..16])
                    .map_err(|error| format!("密钥长度不对：{error}"))?;
            let nonce =
                aes_gcm::Nonce::try_from(nonce).map_err(|_| "nonce 长度不对".to_string())?;
            cipher
                .decrypt(&nonce, payload)
                .map_err(|_| "口令不对，或者内容被改过".to_string())
        }
    }
}

/// 一次同步的结果（给界面看）
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct SyncReport {
    pub uploaded: usize,
    pub downloaded: usize,
    /// 云端跟着删掉的
    pub removed_remote: usize,
    /// 本机跟着删掉的（别的机器删过）
    pub removed_local: usize,
    /// 两边都改过、按时间取了新版的那些（路径 + 取了哪边）
    pub conflicts: Vec<Conflict>,
    pub bytes_up: u64,
    pub bytes_down: u64,
}

/// 一次"两边都改过"的取舍
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Conflict {
    pub path: String,
    /// 取了哪一边：`local` / `remote`
    pub kept: String,
}

/// 跑到哪儿了（界面据此写进度）
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Progress {
    /// `lock` / `upload` / `download` / `done`
    pub phase: String,
    pub done: usize,
    pub total: usize,
    /// 一句人话（界面上直接显示）
    pub text: String,
}

// ---------------------------------------------------------------- 设置与索引

/// 同步的设置文件（`settings/sync.json`）
fn settings_path(workspace: &Workspace) -> PathBuf {
    workspace.settings_file("sync.json")
}

/// 记账本现在的样子：对上了几份、什么时候记的（诊断页要报）
pub struct IndexFacts {
    /// 索引里记着几份文件（"上次对齐时它们是什么样"）
    pub files: usize,
    /// 这个文件最后一次改动是什么时候（RFC3339）；还没有就是空串
    pub updated: String,
}

/// 读一眼记账本（读不动就是空的，不报错 —— 诊断页不该因为读不出来就打不开）
pub fn index_facts(workspace: &Workspace) -> IndexFacts {
    let path = index_path(workspace);
    let index = read_json::<Index>(&path);

    let updated = std::fs::metadata(&path)
        .and_then(|meta| meta.modified())
        .ok()
        .map(|at| {
            OffsetDateTime::from(at)
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap_or_default()
        })
        .unwrap_or_default();

    IndexFacts {
        files: index.files.len(),
        updated,
    }
}

/// 对齐索引（`settings/sync-index.json`）
fn index_path(workspace: &Workspace) -> PathBuf {
    workspace.settings_file("sync-index.json")
}

pub fn settings(workspace: &Workspace) -> SyncSettings {
    read_json(&settings_path(workspace))
}

pub fn save_settings(workspace: &Workspace, settings: &SyncSettings) -> Result<(), String> {
    let path = settings_path(workspace);
    write_json(&path, settings)?;
    // 里面是密钥：只让本人读得动
    restrict(&path);
    Ok(())
}

/// 上次对齐时的样子
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Stamp {
    /// 上次对齐时本机这一份的内容指纹
    hash: String,
    /// 本机文件的时间（Unix 秒）—— 现在只用来给人看，判"变没变"看的是指纹
    #[serde(default)]
    mtime: i64,
    /// 云端那一份的 ETag
    etag: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
struct Index {
    /// 相对路径 → 上次对齐时的样子
    files: BTreeMap<String, Stamp>,
}

// ---------------------------------------------------------------- 一条条规则

/// 本机这一份
#[derive(Debug, Clone, PartialEq)]
struct Local {
    /// 内容指纹：**内容寻址**的那些直接用文件名（名字就是哈希），其余算一遍 sha256。
    ///
    /// 为什么不用"大小 + 修改时间"那种便宜的判法：同一秒里改一笔、正好还一样长，
    /// 那种判法看不出来（测试里就逮到过一次）。这一份仓库不大，算一遍更踏实。
    hash: String,
    mtime: i64,
}

/// 云端这一份
#[derive(Debug, Clone, PartialEq)]
struct Remote {
    size: u64,
    etag: String,
    /// 最后修改时间（Unix 秒）
    modified: i64,
}

/// 云端那份账对某一条怎么说（判断"云端删没删"的证据）
#[derive(Debug, Clone, Copy, PartialEq)]
enum CloudSays {
    /// 云端压根没有这份账（第一次用、换了桶、账本丢了）—— **没有证据**
    NoAccount,
    /// 账上还记着它 —— 清单里没有只是清单的问题
    Listed,
    /// 账上也没有它 —— 两处一致，确实是被删了
    Gone,
}

/// 判"删没删"要的两处证据
#[derive(Debug, Clone, Copy, PartialEq)]
struct Evidence {
    cloud: CloudSays,
    /// 本机这边**留着删除凭据**吗（笔记的日志进了回收站）
    trashed: bool,
}

impl Evidence {
    /// 什么凭据都没有时的那一份
    fn none() -> Self {
        Self {
            cloud: CloudSays::NoAccount,
            trashed: false,
        }
    }
}

/// 一个路径该怎么办
#[derive(Debug, Clone, PartialEq)]
enum Decision {
    /// 两边一样，只把索引擦新
    Nothing,
    Upload,
    Download,
    /// 本机删过 → 云端也删
    DeleteRemote,
    /// 云端删过 → 本机也删
    DeleteLocal,
    /// 两边都动过：按时间取新的那一边
    TakeNewer(&'static str),
}

/// 规则都在这里：给定"本机有没有、云端有没有、上次对齐是什么样"，决定怎么办。
///
/// 单独写成一个纯函数，是为了能**不碰网络**把每条分支都试一遍 —— 同步最怕的就是
/// "某种情形下悄悄丢了东西"，而那只能靠把这些情形一个个列出来盯着。
fn decide(
    force_upload: bool,
    local: Option<&Local>,
    remote: Option<&Remote>,
    aligned: Option<&Stamp>,
    evidence: Evidence,
) -> (Decision, String) {
    // 换过钥匙的那一趟：本机有的一律重传（云端那些旧密文已经解不开了）
    if force_upload {
        return match (local, remote) {
            (Some(_), _) => (Decision::Upload, "重传这一份".to_string()),
            (None, Some(_)) => (Decision::Download, String::new()),
            (None, None) => (Decision::Nothing, String::new()),
        };
    }

    match (local, remote, aligned) {
        // ---- 两边都有 ----
        (Some(local), Some(remote), aligned) => {
            // "变没变"看两样：本机看（大小、时间），云端看 ETag
            let local_changed = aligned
                .map(|stamp| stamp.hash != local.hash)
                .unwrap_or(true);
            // 比 ETag 不认大小写：它是十六进制，真变了不会只差大小写；而不同的接口
            // （清单 / GET / HEAD）偶尔就是这么点差别 —— 认成"变了"就会每趟重下一遍
            let remote_changed = aligned
                .map(|stamp| !stamp.etag.eq_ignore_ascii_case(&remote.etag))
                .unwrap_or(true);

            match (local_changed, remote_changed) {
                (false, false) => (Decision::Nothing, String::new()),
                (true, false) => (Decision::Upload, "本机这份变了".to_string()),
                (false, true) => (Decision::Download, "云端这份变了".to_string()),
                (true, true) => {
                    // 两边都动过：内容块不会走到这里（内容寻址），
                    // 剩下的用时间定；时间也一样时**偏保守**，留本机的那份
                    if remote.modified > local.mtime + 1 {
                        (Decision::TakeNewer("remote"), "云端那份更新".to_string())
                    } else {
                        (Decision::TakeNewer("local"), "本机这份不旧".to_string())
                    }
                }
            }
        }

        // ---- 只有本机有 ----
        (Some(local), None, aligned) => match aligned {
            // 上次对齐时云端有它、现在清单里没了 —— **先别急着删**。
            //
            // "清单里没有"有三种可能：别的机器删了、清单没列全、桶被清空/换了。
            // 只有**云端那份账也记着它没了**，才是第一种。分不清就别删 ——
            // "本地新写的东西被当成旧版本删掉"就是这么来的。
            Some(stamp)
                if stamp.hash == local.hash && evidence.cloud == CloudSays::Gone =>
            {
                (Decision::DeleteLocal, "云端已经删掉".to_string())
            }
            Some(stamp) if stamp.hash == local.hash => (
                Decision::Upload,
                "云端清单里没有它，账上还在 —— 当作没传上去".to_string(),
            ),
            Some(_) => (Decision::Upload, "云端删过，但本机这份改过".to_string()),
            // 从来没有过：新写的，传上去
            None => (Decision::Upload, String::new()),
        },

        // ---- 只有云端有 ----
        (None, Some(_), aligned) => match aligned {
            // 本机删过 —— 但**"本机没有"和"我删的"是两回事**：删除会留下凭据
            // （笔记的日志进回收站，见 `trashed_at`）。有凭据才替人删云端那份；
            // 没有就取回来 —— 宁可多一份，也别把云端唯一的那一份抹掉。
            Some(_) if evidence.trashed => {
                (Decision::DeleteRemote, "本机已经删掉".to_string())
            }
            Some(_) => (
                Decision::Download,
                "云端还有这一份、本机没了 —— 取回来（要删它请在本机删）".to_string(),
            ),
            None => (Decision::Download, String::new()),
        },

        // ---- 两边都没有 ----
        (None, None, _) => match aligned {
            // 索引里有、两边都没了：擦掉这条记录
            Some(_) => (Decision::Nothing, String::new()),
            None => (Decision::Nothing, String::new()),
        },
    }
}

// ---------------------------------------------------------------- 跑一次

/// 同步跑起来的入口：抢锁、对账、动手、放锁。
pub fn run(
    workspace: &Workspace,
    settings: &SyncSettings,
    progress: &dyn Fn(Progress),
    force: bool,
) -> Result<SyncReport, String> {
    if !settings.is_ready() {
        return Err("同步没开着，或者 S3 的配置还没填全".to_string());
    }
    // 加密开着、钥匙也在：传上去的每一份都过一道；否则原样
    let transform: Box<dyn SyncTransform> = if settings.encrypt {
        Box::new(settings.cipher()?)
    } else {
        Box::new(Plaintext)
    };
    let s3 = S3::new(settings.s3.clone())?;
    let root = workspace.root().to_path_buf();
    let mut index = read_json::<Index>(&index_path(workspace));

    // ---- 抢锁 ----
    progress(Progress {
        phase: "lock".to_string(),
        done: 0,
        total: 1,
        text: if force {
            "正在抢云端同步锁…".to_string()
        } else {
            "正在取得云端同步锁…".to_string()
        },
    });
    let lock = acquire(&s3, force)?;

    // ---- 云端那份账本 ----
    //
    // 本地这份记的是"**这台机器**上次对齐时什么样"。它要是空的（新装的机器、
    // 换过桶、被清理过），而云端有一份，那就照云端的认路：不然云端已有的每一份
    // 都会被当成"本机新写的"再传一遍 —— 那条路上出过"新仓库盖掉云端"的事。
    let cloud = fetch_latest(&s3);
    if index.files.is_empty() {
        if let Some(cloud) = &cloud {
            if !cloud.files.is_empty() {
                progress(Progress {
                    phase: "lock".to_string(),
                    done: 0,
                    total: 1,
                    text: "本机没有账本，照云端的认路…".to_string(),
                });
                index = cloud.clone();
            }
        }
    }

    // 干活期间续锁 —— 顺便搭在进度回调上，不必另开线程（见 [`LOCK_REFRESH`]）
    let last_touch = Cell::new(Instant::now());
    let beating = |step: Progress| {
        if last_touch.get().elapsed() >= LOCK_REFRESH {
            last_touch.set(Instant::now());
            let _ = lock.touch(&s3);
        }
        progress(step);
    };

    let outcome = reconcile(
        &s3,
        &root,
        workspace,
        &mut index,
        cloud.as_ref(),
        transform.as_ref(),
        settings.reupload,
        &beating,
    );

    // 不管成没成，锁都要放掉：留着它，别的机器要等过期才能动
    let _ = lock.release(&s3);

    let report = outcome?;
    index.save(workspace)?;

    // 云端那份账本跟着更新：它记的是"跑完这一趟之后，哪些已经对齐"。
    // 写不上去不算这一趟失败（东西都传完了），只记一笔 —— 账本旧了是**少删**，
    // 不是乱删，方向是安全的。
    if let Err(error) = publish_latest(&s3, &index) {
        eprintln!("[sync] 云端账本没写上去：{error}");
    }

    // "整份重传"是**一次性**的：这一趟跑完就清掉，免得此后每次都重传
    if settings.reupload {
        let mut updated = settings.clone();
        updated.reupload = false;
        save_settings(workspace, &updated)?;
    }
    Ok(report)
}

/// 对账 + 动手（锁已经拿到了）
fn reconcile(
    s3: &S3,
    root: &Path,
    workspace: &Workspace,
    index: &mut Index,
    // 云端那份账（没有就是 `None`）—— 判"云端删没删"要它当证据
    cloud: Option<&Index>,
    transform: &dyn SyncTransform,
    force_upload: bool,
    progress: &dyn Fn(Progress),
) -> Result<SyncReport, String> {
    let mut report = SyncReport::default();

    // ---- 两边的清单 ----
    let local = scan_local(root)?;
    let remote: BTreeMap<String, Remote> = s3
        .list(&prefix_of(s3))
        .map_err(|error| format!("列不出云端清单：{error}"))?
        .into_iter()
        .filter_map(|object| {
            let relative = relative_of(s3, &object.key)?;
            Some((
                relative,
                Remote {
                    size: object.size,
                    etag: object.etag,
                    modified: parse_time(&object.modified)?,
                },
            ))
        })
        .collect();

    // ---- 一条条比 ----
    let mut paths: Vec<String> = local.keys().cloned().collect();
    paths.extend(remote.keys().cloned());
    paths.sort_unstable();
    paths.dedup();

    // 这一趟要从云端取回日志吗（云端有、本机没有的日志）—— 它决定内容块能不能删：
    // 那些日志可能正指着这些内容块，删了就"页面在、内容没了"。
    let restoring_logs = remote
        .keys()
        .any(|path| path.starts_with("db/objects/") && !local.contains_key(path));

    // 决定怎么办（纯规则）—— 判"删没删"要两处证据：云端那份账怎么说（`cloud`），
    // 以及本机这边有没有留下删除凭据（笔记的日志会进回收站）
    let planned: Vec<(String, Decision, String)> = paths
        .iter()
        .map(|path| {
            let evidence = Evidence {
                cloud: match cloud {
                    Some(cloud) if cloud.files.contains_key(path) => CloudSays::Listed,
                    Some(_) => CloudSays::Gone,
                    None => CloudSays::NoAccount,
                },
                // 内容块没有回收站：本机删它只有一条正当路径 —— 整理（GC）发现没人引用它。
                // 认不出"是整理删的"还是"工作目录被清过"，那就退一步问：
                // **本机缺着云端有的日志吗？** 缺，就说明这一趟要把日志取回来，
                // 而那些日志可能正指着这些内容块 —— 一个都不能删（取回来才对）。
                trashed: trashed_at(root, path)
                    || (path.starts_with("db/blobs/") && !restoring_logs),
            };
            let (decision, why) = decide(
                force_upload,
                local.get(path),
                remote.get(path),
                index.files.get(path),
                evidence,
            );
            (path.clone(), decision, why)
        })
        .collect();

    let work: Vec<&(String, Decision, String)> = planned
        .iter()
        .filter(|(_, decision, _)| !matches!(decision, Decision::Nothing))
        .collect();
    let total = work.len();

    // ---- 动手 ----
    for (done, (path, decision, why)) in work.iter().enumerate() {
        let text = describe(path, decision, why);
        progress(Progress {
            phase: match decision {
                Decision::Upload | Decision::DeleteRemote => "upload",
                _ => "download",
            }
            .to_string(),
            done,
            total,
            text,
        });

        // 隔一段把记账落一次盘（见 `INDEX_SAVE_EVERY`）
        if total > INDEX_SAVE_EVERY && done > 0 && done % INDEX_SAVE_EVERY == 0 {
            let _ = index.save(workspace);
        }

        match decision {
            Decision::Upload | Decision::TakeNewer("local") => {
                let bytes =
                    fs::read(root.join(path)).map_err(|error| format!("读不出 {path}：{error}"))?;
                let key = s3.config().key_of(path);
                let sealed = transform.seal(path, bytes)?;
                s3.put(&key, &sealed, false)
                    .map_err(|error| format!("传 {path} 失败：{error}"))?;
                // 传完记下云端的 ETag：下次比的就是它
                let etag = s3
                    .head(&key)
                    .map_err(|error| format!("看 {path} 失败：{error}"))?
                    .map(|object| object.etag)
                    .unwrap_or_default();
                report.uploaded += 1;
                report.bytes_up += sealed.len() as u64;
                stamp_local(index, root, path, etag);
                if matches!(decision, Decision::TakeNewer(_)) {
                    report.conflicts.push(Conflict {
                        path: path.clone(),
                        kept: "local".to_string(),
                    });
                }
            }

            Decision::Download | Decision::TakeNewer("remote") => {
                let key = s3.config().key_of(path);
                let Some(fetched) = s3
                    .get(&key)
                    .map_err(|error| format!("取 {path} 失败：{error}"))?
                else {
                    continue;
                };
                let plain = transform.open(path, fetched.bytes)?;
                write_local(root, path, &plain)?;
                report.downloaded += 1;
                report.bytes_down += plain.len() as u64;
                // 记的是**清单**里的那个 ETag，不是这次 GET 响应头里的：下一趟比的就是
                // 清单，两边得是同一路读来的。记成另一路的，那两路只要有一点出入
                // （引号写法、大小写、有没有这个头），每趟都会判"云端变了" —— 于是
                // 下载完再点同步，又是全量下载一遍。
                let etag = remote
                    .get(path)
                    .map(|held| held.etag.clone())
                    .unwrap_or_else(|| fetched.etag.clone());
                stamp_local(index, root, path, etag);
                if matches!(decision, Decision::TakeNewer(_)) {
                    report.conflicts.push(Conflict {
                        path: path.clone(),
                        kept: "remote".to_string(),
                    });
                }
            }

            Decision::DeleteRemote => {
                s3.delete(&s3.config().key_of(path))
                    .map_err(|error| format!("删云端 {path} 失败：{error}"))?;
                report.removed_remote += 1;
                index.files.remove(path);
            }

            Decision::DeleteLocal => {
                let _ = fs::remove_file(root.join(path));
                report.removed_local += 1;
                index.files.remove(path);
                let _ = why;
            }

            // `TakeNewer` 的另一半在下面那两支里处理了；这里不该再走到
            Decision::TakeNewer(_) | Decision::Nothing => {}
        }
    }

    progress(Progress {
        phase: "done".to_string(),
        done: total,
        total,
        text: "同步完成".to_string(),
    });

    Ok(report)
}

fn describe(path: &str, decision: &Decision, why: &str) -> String {
    let what = match decision {
        Decision::Upload | Decision::TakeNewer("local") => format!("上传 {path}"),
        Decision::Download | Decision::TakeNewer("remote") => format!("下载 {path}"),
        Decision::DeleteRemote => format!("云端删除 {path}"),
        Decision::DeleteLocal => format!("本机删除 {path}"),
        _ => path.to_string(),
    };
    if why.is_empty() {
        what
    } else {
        format!("{what}（{why}）")
    }
}

/// 记下这一份现在的样子（内容指纹、时间、云端的 ETag）
fn stamp_local(index: &mut Index, root: &Path, path: &str, etag: String) {
    let target = root.join(path);
    let Ok(meta) = fs::metadata(&target) else {
        return;
    };
    let hash = if path.starts_with("db/blobs/") {
        path.rsplit('/').next().unwrap_or_default().to_string()
    } else {
        content_hash(&target)
    };
    index.files.insert(
        path.to_string(),
        Stamp {
            hash,
            mtime: mtime_of(&meta),
            etag,
        },
    );
}

/// 一份文件的内容指纹（内容块不必走这里，它们的名字就是指纹）
fn content_hash(path: &Path) -> String {
    use sha2::{Digest, Sha256};

    match fs::read(path) {
        Ok(bytes) => Sha256::digest(&bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
        Err(_) => String::new(),
    }
}

/// 写回本机（顺带把目录建出来 —— 云端有的那些目录，本机可能是空的）
fn write_local(root: &Path, path: &str, bytes: &[u8]) -> Result<(), String> {
    let target = root.join(path);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("建不出 {}：{error}", parent.display()))?;
    }
    crate::storage::workspace::write_bytes(&target, bytes)
        .map_err(|error| format!("写 {path} 失败：{error}"))
}

/// 本机这一份清单：只挑**该同步**的那些
fn scan_local(root: &Path) -> Result<BTreeMap<String, Local>, String> {
    let mut out = BTreeMap::new();
    walk(root, root, &mut out)?;
    Ok(out)
}

fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, Local>) -> Result<(), String> {
    let entries =
        fs::read_dir(dir).map_err(|error| format!("读不出 {}：{error}", dir.display()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            walk(root, &path, out)?;
            continue;
        }
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        let relative = relative.to_string_lossy().replace('\\', "/");
        if !is_synced(&relative) {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        let hash = if relative.starts_with("db/blobs/") {
            // 内容寻址：名字就是指纹，不必读内容
            relative.rsplit('/').next().unwrap_or_default().to_string()
        } else {
            content_hash(&path)
        };
        out.insert(
            relative,
            Local {
                hash,
                mtime: mtime_of(&meta),
            },
        );
    }
    Ok(())
}

/// 该同步的是哪些 —— 这个问题由**工作目录**那一层回答（见
/// [`crate::storage::workspace::is_synced`]）：工作目录长什么样是它的事，
/// 同步只管照办。往后加东西时也就不必来改这个模块。
fn is_synced(relative: &str) -> bool {
    crate::storage::workspace::is_synced(relative)
}

fn prefix_of(s3: &S3) -> String {
    let prefix = s3.config().prefix.trim_matches('/');
    if prefix.is_empty() {
        String::new()
    } else {
        format!("{prefix}/")
    }
}

/// 云端的键 → 工作目录里的相对路径；不该同步的那些返回 `None`
fn relative_of(s3: &S3, key: &str) -> Option<String> {
    let prefix = prefix_of(s3);
    let relative = key.strip_prefix(&prefix)?;
    // 锁与账本是云端自己的东西，不是仓库里的文件 —— 别把它们下载到工作目录里
    if relative.is_empty()
        || relative == LOCK_KEY
        || relative == LATEST_KEY
        || !is_synced(relative)
    {
        return None;
    }
    Some(relative.to_string())
}

/// 读云端那份账本（没有就是 `None`：第一次用、换了桶、或者还没写过）
fn fetch_latest(s3: &S3) -> Option<Index> {
    let key = s3.config().key_of(LATEST_KEY);
    let fetched = s3.get(&key).ok().flatten()?;
    serde_json::from_slice::<Index>(&fetched.bytes).ok()
}

/// 把跑完时的账本写到云端。
///
/// **明文**：里面只有路径、内容指纹与 ETag，没有一分内容；而路径本来就以键的形式
/// 摆在云端（键名就是相对路径），再封一层只是自欺欺人。
fn publish_latest(s3: &S3, index: &Index) -> Result<(), String> {
    let key = s3.config().key_of(LATEST_KEY);
    let body = serde_json::to_vec(index).map_err(|error| format!("账本序列化失败：{error}"))?;
    s3.put(&key, &body, false)?;
    Ok(())
}

/// 本机这边"这一份是被删掉的"凭据：笔记的日志进了回收站。
///
/// 只有 `db/objects/<命名空间>/<id>.log` 这种形状查得出来（回收站按命名空间存，
/// 见 `Database::trash_note_path`）。别的形状一律"不知道" —— 不知道就别替人删
/// （内容块那一类由 `reconcile` 另作判断，见那里的说明）。
fn trashed_at(root: &Path, path: &str) -> bool {
    let Some(rest) = path.strip_prefix("db/objects/") else {
        return false;
    };
    root.join("trash").join(rest).is_file()
}

fn mtime_of(meta: &fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|since| since.as_secs() as i64)
        .unwrap_or_default()
}

/// `2026-10-01T10:00:00.000Z` → Unix 秒；认不出就是 0（那一边就按"旧"处理）
fn parse_time(text: &str) -> Option<i64> {
    OffsetDateTime::parse(text, &time::format_description::well_known::Rfc3339)
        .ok()
        .map(|time| time.unix_timestamp())
}

impl Index {
    fn save(&self, workspace: &Workspace) -> Result<(), String> {
        write_json(&index_path(workspace), self)
    }
}

// ---------------------------------------------------------------- 云端那把锁

/// 拿在手里的锁
struct Lock {
    key: String,
}

impl Lock {
    fn release(self, s3: &S3) -> Result<(), String> {
        s3.delete(&self.key)
    }

    /// 续一下：把时间戳写成现在（长活儿跑到一半用，见 [`LOCK_REFRESH`]）。
    ///
    /// 别的机器据此知道这边还活着 —— 一趟大同步不至于被当成"崩了"抢走。
    fn touch(&self, s3: &S3) -> Result<(), String> {
        let body = lock_body()?;
        s3.put(&self.key, &body, false)?;
        Ok(())
    }
}

/// 锁里写的是什么（拿锁与续锁写的是同一份东西）
fn lock_body() -> Result<Vec<u8>, String> {
    let now = OffsetDateTime::now_utc().unix_timestamp();
    serde_json::to_vec(&LockBody {
        host: hostname(),
        at: now,
    })
    .map_err(|error| format!("锁序列化失败：{error}"))
}

/// 锁里写着什么（过期判断要读它）
#[derive(Debug, Clone, Serialize, Deserialize)]
struct LockBody {
    /// 谁拿的（机器名，报告里给另一台看）
    host: String,
    /// 什么时候拿的（Unix 秒）
    at: i64,
}

/// 抢锁：`If-None-Match: *` 是 S3 原生的"不存在才写得进去"。
///
/// 已经有人拿着、而且没超过 [`LOCK_TTL`]：报错让这次同步别跑 —— 两台机器同时改
/// 会互相盖掉。过期了就抢过来（机器崩了不该把同步永久锁死）。
///
/// `force` 是**人**按下去的"不等了"（界面上那颗「强制同步」）：锁还热着也照样盖成
/// 自己的。另一台机器要是真在同步，两边就撞上了 —— 所以默认不走这条路，界面上也
/// 写着代价（宁可等它超时）。
fn acquire(s3: &S3, force: bool) -> Result<Lock, String> {
    let key = s3.config().key_of(LOCK_KEY);
    let body = lock_body()?;

    if force {
        s3.put(&key, &body, false)?;
        return Ok(Lock { key });
    }

    let now = OffsetDateTime::now_utc().unix_timestamp();
    match s3.put(&key, &body, true)? {
        crate::storage::s3::PutOutcome::Written => Ok(Lock { key }),
        crate::storage::s3::PutOutcome::AlreadyExists | crate::storage::s3::PutOutcome::Changed => {
            // 有人拿着：看看过期没有
            let held = s3
                .get(&key)?
                .and_then(|fetched| serde_json::from_slice::<LockBody>(&fetched.bytes).ok());
            let stale = held
                .as_ref()
                .map(|body| now - body.at > LOCK_TTL)
                .unwrap_or(true);

            let who = held
                .as_ref()
                .map(|body| body.host.clone())
                .unwrap_or_else(|| "另一台机器".to_string());
            if !stale {
                return Err(format!(
                    "云端同步锁正被「{who}」拿着（它要是崩了，{LOCK_TTL} 秒后自动过期）；\
                     不想等就在设置页点「强制同步」抢过来"
                ));
            }

            // 过期的：删掉再抢一次。删与抢之间可能有人插进来 —— 那正好，他拿走便是
            s3.delete(&key)?;
            match s3.put(&key, &body, true)? {
                crate::storage::s3::PutOutcome::Written => Ok(Lock { key }),
                _ => Err("云端同步锁刚好被别的机器抢走了，过一会儿再试".to_string()),
            }
        }
    }
}

fn hostname() -> String {
    // 三个系统三套说法：Linux 认 `HOSTNAME`、Windows 认 `COMPUTERNAME`，
    // 再不行（比如 Android）读 `/etc/hostname`，都不行就报"某台机器" —— 锁上那个
    // 名字只是给另一台机器看的，没有它也一样排队。
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .or_else(|_| std::fs::read_to_string("/etc/hostname").map(|text| text.trim().to_string()))
        .ok()
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "某台机器".to_string())
}

/// 密钥只让本人读得动
fn restrict(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local(hash: &str, mtime: i64) -> Local {
        Local {
            hash: hash.to_string(),
            mtime,
        }
    }

    fn remote(etag: &str, modified: i64) -> Remote {
        Remote {
            size: 10,
            etag: etag.to_string(),
            modified,
        }
    }

    fn stamp(hash: &str, mtime: i64, etag: &str) -> Stamp {
        Stamp {
            hash: hash.to_string(),
            mtime,
            etag: etag.to_string(),
        }
    }

    /// **交给界面的那一份里不许有秘密**：密钥与 S3 私钥都只报"有没有"。
    ///
    /// 这条是给后来者看的护栏：哪天有人图省事把 `key` 加回 view，测试当场就红。
    #[test]
    fn the_view_carries_no_secrets() {
        let settings = SyncSettings {
            enabled: true,
            encrypt: true,
            cipher: crate::storage::codec::Cipher::Aes256Gcm,
            reupload: false,
            key: "SUPER-SECRET-CLOUD-KEY".to_string(),
            s3: S3Config {
                endpoint: "https://s3.example.com".to_string(),
                bucket: "b".to_string(),
                access_key: "AK".to_string(),
                secret_key: "SUPER-SECRET-S3".to_string(),
                ..Default::default()
            },
        };

        let shown = serde_json::to_string(&settings.view()).unwrap();
        assert!(!shown.contains("SUPER-SECRET"), "{shown}");
        assert!(shown.contains("\"has_key\":true"), "{shown}");
        assert!(shown.contains("\"has_secret\":true"), "{shown}");

        // 界面交回来时私钥留空 = **不改**（它本来就拿不到原来那一把）
        let mut target = settings.clone();
        target.apply(SyncSettingsPatch {
            enabled: false,
            encrypt: true,
            cipher: crate::storage::codec::Cipher::Aes256Gcm,
            endpoint: "https://other".to_string(),
            ..Default::default()
        });
        assert_eq!(target.s3.secret_key, "SUPER-SECRET-S3");
        assert_eq!(target.s3.endpoint, "https://other");
        assert_eq!(target.key, "SUPER-SECRET-CLOUD-KEY");
    }

    /// 云端那一层：封了能解、封出来的每一次都不一样、换了钥匙解不开
    #[test]
    fn the_cloud_layer_round_trips() {
        let key = generate_key().unwrap();
        let cipher = CloudCipher::from_key(&key).unwrap();

        let sealed = cipher.seal("db/titles.json", b"hello".to_vec()).unwrap();
        assert_ne!(sealed, b"hello", "传上去的不该是明文");
        assert!(CloudEnvelope::matches(&sealed), "自带说明：这是本程序封的");
        assert_eq!(
            cipher.open("db/titles.json", sealed.clone()).unwrap(),
            b"hello"
        );

        // 同一份内容封两次：nonce 不同，密文就不同
        let again = cipher.seal("db/titles.json", b"hello".to_vec()).unwrap();
        assert_ne!(sealed, again);

        // 换一把钥匙：解不开（而且说得清是钥匙的问题）
        let other = CloudCipher::from_key(&generate_key().unwrap()).unwrap();
        let error = other.open("db/titles.json", sealed).unwrap_err();
        assert!(error.contains("密钥"), "{error}");
    }

    /// 没封过的（开加密之前传上去的那些）：原样放行 ——
    /// 换了设置不该把已经躺在云端的东西读废
    #[test]
    fn a_plain_object_still_comes_back() {
        let cipher = CloudCipher::from_key(&generate_key().unwrap()).unwrap();
        let plain = br#"{"kind":"x"}"#.to_vec();
        assert_eq!(
            cipher.open("db/meta.json", plain.clone()).unwrap(),
            plain,
            "不是我们封的，就照原样用"
        );
    }

    /// 抄错的钥匙当场说清楚（少几个字、多几个字都要认得出）
    #[test]
    fn a_mistyped_key_is_refused_early() {
        assert!(CloudCipher::from_key("这不是 base64").is_err());
        assert!(CloudCipher::from_key("AAAA").is_err(), "长度不对也要拦下");
        assert!(CloudCipher::from_key(&generate_key().unwrap()).is_ok());
    }

    #[test]
    fn only_what_belongs_to_the_repository_is_synced() {
        assert!(is_synced("db/blobs/ab/abc"));
        assert!(is_synced("db/objects/0/1.log"));
        assert!(is_synced("db/titles.json"));
        assert!(is_synced("settings/repository.json"));

        // 这台机器自己的东西：不传
        assert!(!is_synced("settings/preferences.json"));
        assert!(!is_synced("settings/browsing.jsonl"));
        assert!(!is_synced("settings/sync.json"));
        assert!(!is_synced("settings/sync-index.json"));
        assert!(!is_synced("db/drafts/abc"));
    }

    /// 没动过的两边（索引也对得上）：什么也不做
    #[test]
    fn a_quiet_pair_does_nothing() {
        let (decision, _) = decide(
            false,
            Some(&local("h1", 100)),
            Some(&remote("aaa", 100)),
            Some(&stamp("h1", 100, "aaa")),
            Evidence::none(),
        );
        assert_eq!(decision, Decision::Nothing);
    }

    #[test]
    fn only_the_local_side_changed_uploads() {
        let (decision, _) = decide(
            false,
            Some(&local("h2", 200)),
            Some(&remote("aaa", 100)),
            Some(&stamp("h1", 100, "aaa")),
            Evidence::none(),
        );
        assert_eq!(decision, Decision::Upload);
    }

    #[test]
    fn only_the_remote_side_changed_downloads() {
        let (decision, _) = decide(
            false,
            Some(&local("h1", 100)),
            Some(&remote("bbb", 200)),
            Some(&stamp("h1", 100, "aaa")),
            Evidence::none(),
        );
        assert_eq!(decision, Decision::Download);
    }

    /// 两边都动过：云端更新就取云端，本机不旧就留本机（**不静默丢东西**，会记一条）
    #[test]
    fn a_real_conflict_is_settled_by_time() {
        let (decision, why) = decide(
            false,
            Some(&local("h2", 200)),
            Some(&remote("bbb", 300)),
            Some(&stamp("h1", 100, "aaa")),
            Evidence::none(),
        );
        assert_eq!(decision, Decision::TakeNewer("remote"));
        assert!(!why.is_empty(), "取哪边、为什么，要说得出来");

        let (decision, _) = decide(
            false,
            Some(&local("h2", 400)),
            Some(&remote("bbb", 300)),
            Some(&stamp("h1", 100, "aaa")),
            Evidence::none(),
        );
        assert_eq!(decision, Decision::TakeNewer("local"));

        // 时间几乎一样时保守：留本机那份（宁可多传一次，也别把刚写的盖掉）
        let (decision, _) = decide(
            false,
            Some(&local("h2", 300)),
            Some(&remote("bbb", 300)),
            Some(&stamp("h1", 100, "aaa")),
            Evidence::none(),
        );
        assert_eq!(decision, Decision::TakeNewer("local"));
    }

    /// **清单空白绝不能当成"云端删过"** —— 这是真出过的事故：
    ///
    /// 桶名被填成了带桶的端点（`https://桶.s3.某云.net`），客户端又按"桶名走路径"拼 URL，
    /// 于是上传把桶名写进了键、列举永远列不到东西。结果每同步一次，就把本机那些
    /// "账上记着已对齐"的文件当成"云端删过的"删掉一批 —— 用户刚写的页面就是这么没的。
    #[test]
    fn a_missing_listing_never_means_the_cloud_deleted_it() {
        let here = local("h1", 100);
        let stamp = stamp("h1", 100, "aaa");

        // 云端账上还记着它（只是清单里没列出来）→ 传上去，绝不删
        let (decision, why) = decide(
            false,
            Some(&here),
            None,
            Some(&stamp),
            Evidence {
                cloud: CloudSays::Listed,
                trashed: false,
            },
        );
        assert_eq!(decision, Decision::Upload, "{why}");

        // 云端连账都没有（第一次用、换了桶）→ 同样没有证据，同样不删
        let (decision, _) = decide(
            false,
            Some(&here),
            None,
            Some(&stamp),
            Evidence::none(),
        );
        assert_eq!(decision, Decision::Upload);
    }

    /// 云端有、本机没有，但**本机这边找不到"是删掉的"凭据** → 取回来，别替人抹掉
    #[test]
    fn a_cloud_file_is_restored_unless_the_deletion_left_a_trace() {
        let there = remote("aaa", 100);

        // 回收站里没有它：取回本机
        let (decision, why) = decide(
            false,
            None,
            Some(&there),
            Some(&stamp("h1", 100, "aaa")),
            Evidence {
                cloud: CloudSays::Gone,
                trashed: false,
            },
        );
        assert_eq!(decision, Decision::Download, "{why}");

        // 回收站里有它：本机确实删过，云端跟着删
        let (decision, _) = decide(
            false,
            None,
            Some(&there),
            Some(&stamp("h1", 100, "aaa")),
            Evidence {
                cloud: CloudSays::Gone,
                trashed: true,
            },
        );
        assert_eq!(decision, Decision::DeleteRemote);
    }

    /// 新写的传上去，云端多出来的拿回来
    #[test]
    fn a_first_sync_takes_the_union() {
        let (decision, _) = decide(false, Some(&local("h1", 100)), None, None,
            Evidence::none(),
        );
        assert_eq!(decision, Decision::Upload);

        let (decision, _) = decide(false, None, Some(&remote("aaa", 100)), None,
            Evidence::none(),
        );
        assert_eq!(decision, Decision::Download);
    }

    /// 删除要**两边都认**、**而且有凭据**才动
    #[test]
    fn deletions_travel_only_when_the_other_side_is_untouched() {
        // 本机删了（回收站里留着凭据）、云端还在 → 云端也删
        let (decision, why) = decide(
            false,
            None,
            Some(&remote("aaa", 100)),
            Some(&stamp("h1", 100, "aaa")),
            Evidence {
                cloud: CloudSays::Gone,
                trashed: true,
            },
        );
        assert_eq!(decision, Decision::DeleteRemote);
        assert!(!why.is_empty());

        // 云端账上也说没了，本机没动过 → 本机也删
        let (decision, _) = decide(
            false,
            Some(&local("h1", 100)),
            None,
            Some(&stamp("h1", 100, "aaa")),
            Evidence {
                cloud: CloudSays::Gone,
                trashed: false,
            },
        );
        assert_eq!(decision, Decision::DeleteLocal);

        // 云端删了、本机**改过** → 不跟着删（本机那份是新的，传上去）
        let (decision, _) = decide(
            false,
            Some(&local("h2", 300)),
            None,
            Some(&stamp("h1", 100, "aaa")),
            Evidence {
                cloud: CloudSays::Gone,
                trashed: false,
            },
        );
        assert_eq!(
            decision,
            Decision::Upload,
            "本机改过的那份不能被云端的删除带走"
        );
    }
}

/// **端到端**：拿一个内存里的假 S3（只认那几个方法）把整条流程走一遍 ——
/// 传上去、把本机删干净、再从云端拿回来。签名另有官方向量盯着，这里验的是"这条路通不通"。
#[cfg(test)]
mod end_to_end {
    use super::*;
    use std::collections::HashMap;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::{Arc, Mutex};

    /// 桶里的东西：键 → (内容, ETag)
    type Bucket = Arc<Mutex<HashMap<String, (Vec<u8>, String)>>>;

    /// 一个假服务：桶 + 开关
    #[derive(Clone)]
    struct Fake {
        bucket: Bucket,
        /// 让**列举**装死（东西还在，就是列不出来）。
        ///
        /// 这一条是从真事上来的：桶名被填进端点（`https://桶.s3.某云.net`）、
        /// 客户端又按"桶名走路径"拼 URL，上传把桶名写进了键，列举则永远列不到东西 ——
        /// 于是"云端一片空白"被读成"云端删过"，把本机的东西删了。
        blind_listing: Arc<std::sync::atomic::AtomicBool>,
    }

    /// 起一个只够测试用的 S3，返回 (地址, 假服务)
    fn start_fake_s3() -> (String, Fake) {
        let fake = Fake {
            bucket: Arc::new(Mutex::new(HashMap::new())),
            blind_listing: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        };
        let listener = TcpListener::bind("127.0.0.1:0").expect("绑定本地端口");
        let address = format!("http://{}", listener.local_addr().unwrap());

        let served = fake.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let _ = serve(stream, &served);
            }
        });
        (address, fake)
    }

    /// 一次请求的应答：状态码、该带的头、正文
    struct Reply {
        status: u16,
        content_type: String,
        /// 这一份的 ETag（**GET 与 LIST 必须说同一个** —— 引擎就是靠它判断
        /// "云端变没变"，两个接口说法不一致会让它以为每次都变了）
        etag: Option<String>,
        body: Vec<u8>,
    }

    fn serve(mut stream: TcpStream, fake: &Fake) -> std::io::Result<()> {
        let mut reader = BufReader::new(stream.try_clone()?);

        // 请求行
        let mut line = String::new();
        reader.read_line(&mut line)?;
        let mut parts = line.split_whitespace();
        let method = parts.next().unwrap_or_default().to_string();
        let target = parts.next().unwrap_or_default().to_string();

        // 头
        let mut headers: HashMap<String, String> = HashMap::new();
        loop {
            let mut header = String::new();
            reader.read_line(&mut header)?;
            let header = header.trim_end();
            if header.is_empty() {
                break;
            }
            if let Some((name, value)) = header.split_once(':') {
                headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
            }
        }

        // 正文
        let length: usize = headers
            .get("content-length")
            .and_then(|value| value.parse().ok())
            .unwrap_or(0);
        let mut body = vec![0u8; length];
        if length > 0 {
            reader.read_exact(&mut body)?;
        }

        let (path, query) = match target.split_once('?') {
            Some((path, query)) => (path.to_string(), query.to_string()),
            None => (target.clone(), String::new()),
        };
        let key = decode(path.trim_start_matches('/'));
        let key = key
            .split_once('/')
            .map(|(_, rest)| rest.to_string())
            .unwrap_or(key);

        let reply = route(&method, &key, &query, &headers, body, fake);

        let mut out = format!(
            "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n",
            reply.status,
            if reply.status == 200 { "OK" } else { "Error" },
            reply.content_type,
            reply.body.len()
        );
        if let Some(etag) = &reply.etag {
            out.push_str(&format!("ETag: \"{etag}\"\r\n"));
        }
        out.push_str("Last-Modified: Wed, 01 Oct 2026 10:00:00 GMT\r\n\r\n");
        stream.write_all(out.as_bytes())?;
        stream.write_all(&reply.body)?;
        Ok(())
    }

    /// 按方法分派（桶里的东西就是这个假服务的全部"状态"）
    fn route(
        method: &str,
        key: &str,
        query: &str,
        headers: &HashMap<String, String>,
        body: Vec<u8>,
        fake: &Fake,
    ) -> Reply {
        let mut map = fake.bucket.lock().unwrap();
        // 假服务里 ETag 只要"内容一样就一样、变了就变"即可，够对账用了
        let etag_of = |bytes: &[u8]| {
            use sha2::{Digest, Sha256};
            let digest = Sha256::digest(bytes);
            digest
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        };

        match method {
            "PUT" => {
                // 条件写：`If-None-Match: *` 只在不认识这个键时才写得进去
                if headers.get("if-none-match").map(|v| v.as_str()) == Some("*")
                    && map.contains_key(key)
                {
                    return Reply {
                        status: 412,
                        content_type: "text/plain".into(),
                        etag: None,
                        body: vec![],
                    };
                }
                let etag = etag_of(&body);
                map.insert(key.to_string(), (body, etag.clone()));
                Reply {
                    status: 200,
                    content_type: "text/plain".into(),
                    etag: Some(etag),
                    body: vec![],
                }
            }
            "GET" if query.contains("list-type=2") => {
                // 装死：东西都在，就是列不出来（见 `Fake::blind_listing`）
                if fake.blind_listing.load(std::sync::atomic::Ordering::SeqCst) {
                    return Reply {
                        status: 200,
                        content_type: "application/xml".into(),
                        etag: None,
                        body: b"<?xml version=\"1.0\"?><ListBucketResult><IsTruncated>false</IsTruncated></ListBucketResult>".to_vec(),
                    };
                }
                let prefix = query
                    .split('&')
                    .find_map(|pair| pair.strip_prefix("prefix="))
                    .map(decode)
                    .unwrap_or_default();
                let mut xml = String::from("<?xml version=\"1.0\"?><ListBucketResult>");
                xml.push_str("<IsTruncated>false</IsTruncated>");
                for (name, (content, etag)) in map.iter() {
                    if !name.starts_with(&prefix) {
                        continue;
                    }
                    // 故意把清单里的 ETag 写成**大写、不带引号**：真有的服务就是这么
                    // 不讲究（清单一路说法、GET/HEAD 另一路）—— 引擎不该因此每趟
                    // 都判"云端变了"，把整份仓库重下一遍。
                    xml.push_str(&format!(
                        "<Contents><Key>{}</Key><LastModified>2026-10-01T10:00:00.000Z</LastModified><ETag>{}</ETag><Size>{}</Size></Contents>",
                        escape(name),
                        etag.to_uppercase(),
                        content.len()
                    ));
                }
                xml.push_str("</ListBucketResult>");
                Reply {
                    status: 200,
                    content_type: "application/xml".into(),
                    etag: None,
                    body: xml.into_bytes(),
                }
            }
            "GET" => match map.get(key) {
                Some((content, etag)) => Reply {
                    status: 200,
                    content_type: "application/octet-stream".into(),
                    etag: Some(etag.clone()),
                    body: content.clone(),
                },
                None => Reply {
                    status: 404,
                    content_type: "text/plain".into(),
                    etag: None,
                    body: vec![],
                },
            },
            "HEAD" => match map.get(key) {
                Some((content, etag)) => Reply {
                    status: 200,
                    content_type: "application/octet-stream".into(),
                    etag: Some(etag.clone()),
                    body: content.clone(),
                },
                None => Reply {
                    status: 404,
                    content_type: "text/plain".into(),
                    etag: None,
                    body: vec![],
                },
            },
            "DELETE" => {
                map.remove(key);
                Reply {
                    status: 204,
                    content_type: "text/plain".into(),
                    etag: None,
                    body: vec![],
                }
            }
            _ => Reply {
                status: 400,
                content_type: "text/plain".into(),
                etag: None,
                body: vec![],
            },
        }
    }

    fn decode(text: &str) -> String {
        let bytes = text.as_bytes();
        let mut out = Vec::new();
        let mut index = 0;
        while index < bytes.len() {
            if bytes[index] == b'%' && index + 2 < bytes.len() {
                let high = (bytes[index + 1] as char).to_digit(16);
                let low = (bytes[index + 2] as char).to_digit(16);
                if let (Some(high), Some(low)) = (high, low) {
                    out.push((high * 16 + low) as u8);
                    index += 3;
                    continue;
                }
            }
            out.push(bytes[index]);
            index += 1;
        }
        String::from_utf8_lossy(&out).to_string()
    }

    fn escape(text: &str) -> String {
        text.replace('&', "&amp;").replace('<', "&lt;")
    }

    /// 一个干净的工作目录（只有该同步的那些文件）
    fn workspace(name: &str) -> Workspace {
        let dir = std::env::temp_dir().join(format!(
            "refind-note-sync-test-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let workspace = Workspace::open(dir).unwrap();
        // db 目录得先在（真正的程序是打开数据库时建的）
        std::fs::create_dir_all(workspace.root().join("db/objects/0")).unwrap();
        std::fs::create_dir_all(workspace.root().join("db/blobs/ab")).unwrap();
        std::fs::create_dir_all(workspace.root().join("db/drafts")).unwrap();
        std::fs::create_dir_all(workspace.root().join("settings")).unwrap();
        workspace
    }

    fn settings_for(address: &str, key: &str) -> SyncSettings {
        SyncSettings {
            enabled: true,
            encrypt: true,
            cipher: crate::storage::codec::Cipher::Aes256Gcm,
            reupload: false,
            key: key.to_string(),
            s3: S3Config {
                endpoint: address.to_string(),
                region: "us-east-1".to_string(),
                bucket: "bucket".to_string(),
                prefix: "notes".to_string(),
                access_key: "AK".to_string(),
                secret_key: "SK".to_string(),
            },
        }
    }

    /// **用户在意的那个流程**：写点东西 → 传上去 → 把本机删干净 → 换台机器拿回来
    #[test]
    fn a_repository_travels_to_the_cloud_and_comes_back() {
        let (address, _bucket) = start_fake_s3();
        let key = generate_key().unwrap();

        // ---- 甲机器：写了两样东西，传上去 ----
        let first = workspace("push");
        std::fs::write(
            first.root().join("db/objects/0/1.log"),
            "{\"rev\":1}\n".as_bytes(),
        )
        .unwrap();
        std::fs::write(first.root().join("db/blobs/ab/abc"), b"PNGDATA").unwrap();
        std::fs::write(first.root().join("settings/repository.json"), b"{\"x\":1}").unwrap();

        let settings = settings_for(&address, &key);
        let report = run(&first, &settings, &|_| {}, false).unwrap();
        assert_eq!(report.uploaded, 3, "三份都该传上去：{report:?}");
        assert_eq!(report.downloaded, 0);

        // 本机自己的东西不该上去
        std::fs::write(first.root().join("settings/preferences.json"), b"{}").unwrap();
        std::fs::write(first.root().join("db/drafts/1"), "写了一半".as_bytes()).unwrap();
        let report = run(&first, &settings, &|_| {}, false).unwrap();
        assert_eq!(
            report.uploaded + report.downloaded,
            0,
            "没改过就什么都不动：{report:?}"
        );

        // ---- 把本机删干净（等于换台机器） ----
        let second = workspace("pull");
        assert!(!second.root().join("db/objects/0/1.log").exists());

        // ---- 乙机器：配置同一个桶、同一把钥匙，同步一次 ----
        let report = run(&second, &settings, &|_| {}, false).unwrap();
        assert_eq!(report.downloaded, 3, "三份都该拿回来：{report:?}");
        assert_eq!(
            std::fs::read_to_string(second.root().join("db/objects/0/1.log")).unwrap(),
            "{\"rev\":1}\n"
        );
        assert_eq!(
            std::fs::read(second.root().join("db/blobs/ab/abc")).unwrap(),
            b"PNGDATA"
        );
        assert_eq!(
            std::fs::read_to_string(second.root().join("settings/repository.json")).unwrap(),
            "{\"x\":1}"
        );

        // 乙机器上没配同步时写下的本机专属文件，不该被传上去
        let report = run(&second, &settings, &|_| {}, false).unwrap();
        assert_eq!(
            report.uploaded + report.downloaded,
            0,
            "第二次同步应当无事可做：{report:?}"
        );

        let _ = std::fs::remove_dir_all(first.root());
        let _ = std::fs::remove_dir_all(second.root());
    }

    /// 钥匙不对：说得清是钥匙的问题（而不是"文件坏了"）
    #[test]
    fn the_wrong_key_says_so() {
        let (address, _bucket) = start_fake_s3();

        let first = workspace("key-a");
        std::fs::write(first.root().join("db/objects/0/1.log"), b"secret").unwrap();
        let settings = settings_for(&address, &generate_key().unwrap());
        run(&first, &settings, &|_| {}, false).unwrap();

        // 换台机器、抄错了钥匙
        let second = workspace("key-b");
        let wrong = settings_for(&address, &generate_key().unwrap());
        let error = run(&second, &wrong, &|_| {}, false).unwrap_err();
        assert!(error.contains("密钥"), "{error}");
        assert!(!second.root().join("db/objects/0/1.log").exists());

        let _ = std::fs::remove_dir_all(first.root());
        let _ = std::fs::remove_dir_all(second.root());
    }

    /// 两台机器都改过同一份：按时间取新的那份，并且**报告里说一声**
    #[test]
    fn a_conflict_is_reported_not_swallowed() {
        let (address, _bucket) = start_fake_s3();
        let key = generate_key().unwrap();

        let first = workspace("conflict-a");
        std::fs::write(first.root().join("db/titles.json"), b"{\"from\":\"a\"}").unwrap();
        let settings = settings_for(&address, &key);
        run(&first, &settings, &|_| {}, false).unwrap();

        let second = workspace("conflict-b");
        run(&second, &settings, &|_| {}, false).unwrap();

        // 乙机器改成"本机这份不旧"的样子（时间往后挪一点，甲机器那份会更旧）
        std::fs::write(second.root().join("db/titles.json"), b"{\"from\":\"b\"}").unwrap();
        // 同时让云端那份也变（拿"本地改过、云端改过"这条分支）
        let third = workspace("conflict-c");
        run(&third, &settings, &|_| {}, false).unwrap();
        std::fs::write(third.root().join("db/titles.json"), b"{\"from\":\"c\"}").unwrap();
        run(&third, &settings, &|_| {}, false).unwrap();

        let report = run(&second, &settings, &|_| {}, false).unwrap();
        assert_eq!(report.conflicts.len(), 1, "两边都改过要报出来：{report:?}");
        assert_eq!(report.conflicts[0].path, "db/titles.json");

        let _ = std::fs::remove_dir_all(first.root());
        let _ = std::fs::remove_dir_all(second.root());
        let _ = std::fs::remove_dir_all(third.root());
    }

    /// 两边的 ETag 只差大小写不算"变了" —— 不然就是每趟全量重下。
    ///
    /// 这一条是从真事上来的：清单与 GET/HEAD 是两条路，服务对同一个 ETag 的写法
    /// 偶尔不一致，而"云端变没变"全靠它比。
    #[test]
    fn an_etag_that_only_differs_in_case_is_not_a_change() {
        let stamp = Stamp {
            hash: "h".to_string(),
            mtime: 100,
            etag: "9F2C4A".to_string(),
        };
        let here = Local {
            hash: "h".to_string(),
            mtime: 100,
        };
        let there = Remote {
            size: 10,
            etag: "9f2c4a".to_string(),
            modified: 100,
        };
        let (decision, _) = decide(false, Some(&here), Some(&there), Some(&stamp),
            Evidence::none(),
        );
        assert_eq!(decision, Decision::Nothing, "只差大小写不算云端变了");
    }

    /// 云端那一层认设置里的算法：选了国密，传上去的就是国密的封装
    ///
    /// 顺带钉住一件要紧的事：**换算法不必换钥匙、也不必重传** ——
    /// 每一份封装的头里记着自己那一档，换回 AES 之后老对象照旧解得开。
    #[test]
    fn the_cloud_layer_uses_the_chosen_cipher() {
        let (address, bucket) = start_fake_s3();
        let key = generate_key().unwrap();

        let first = workspace("cipher-sm4");
        std::fs::write(first.root().join("db/objects/0/1.log"), "{\"rev\":1}\n".as_bytes())
            .unwrap();

        let mut settings = settings_for(&address, &key);
        settings.cipher = crate::storage::codec::Cipher::Sm4Gcm;
        assert_eq!(settings.view().cipher, crate::storage::codec::Cipher::Sm4Gcm);
        run(&first, &settings, &|_| {}, false).unwrap();

        // 云端那份的封装头上记着用的是哪一档（magic(4) + 版本(1) + 算法(1)，见 CloudEnvelope）
        let stored = bucket.bucket.lock().unwrap()["notes/db/objects/0/1.log"].0.clone();
        assert_eq!(&stored[..4], b"RNDS", "应当是封装过的");
        assert_eq!(stored[5], 2, "算法那一字节：2 = 国密 SM4");

        // 换台机器、换回 AES：老那份（国密的）照旧解得开
        let second = workspace("cipher-aes");
        let mut settings = settings_for(&address, &key);
        settings.cipher = crate::storage::codec::Cipher::Aes256Gcm;
        let report = run(&second, &settings, &|_| {}, false).unwrap();
        assert_eq!(report.downloaded, 1, "该把那一份拿回来：{report:?}");
        assert_eq!(
            std::fs::read_to_string(second.root().join("db/objects/0/1.log")).unwrap(),
            "{\"rev\":1}\n"
        );

        let _ = std::fs::remove_dir_all(first.root());
        let _ = std::fs::remove_dir_all(second.root());
    }

    /// **清单一片空白时，什么都不能删** —— 这是真出过的事故（端到端复现）。
    ///
    /// 用户那台机器上：桶名被填进了端点、客户端又按"桶名走路径"拼 URL，
    /// 于是东西都传上去了（PUT/HEAD 自洽，看着成功），可**列举**永远列不到 ——
    /// 每同步一次，就把本机"账上记着已对齐"的文件当成"云端删过的"删掉一批。
    #[test]
    fn a_blind_listing_deletes_nothing_at_all() {
        let (address, fake) = start_fake_s3();
        let key = generate_key().unwrap();
        let machine = workspace("blind");
        std::fs::write(
            machine.root().join("db/objects/0/1.log"),
            "{\"rev\":1}\n".as_bytes(),
        )
        .unwrap();
        std::fs::write(machine.root().join("db/blobs/ab/abc"), b"PNGDATA").unwrap();
        let settings = settings_for(&address, &key);

        // 第一趟：正常传上去（账本也写上去）
        let report = run(&machine, &settings, &|_| {}, false).unwrap();
        assert_eq!(report.uploaded + report.downloaded, 2, "{report:?}");

        // 第二趟：列举装死（东西其实都在桶里）
        fake.blind_listing
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let report = run(&machine, &settings, &|_| {}, false).unwrap();

        assert!(
            machine.root().join("db/objects/0/1.log").is_file(),
            "本机这一份必须还在：{report:?}"
        );
        assert!(
            machine.root().join("db/blobs/ab/abc").is_file(),
            "内容块也要还在：{report:?}"
        );
        assert_eq!(report.removed_local, 0, "不许删本机：{report:?}");
        assert_eq!(report.removed_remote, 0, "也不许删云端：{report:?}");

        let _ = std::fs::remove_dir_all(machine.root());
    }

    /// 新机器（本地没有账本）照着云端那份认路：不重传，也不乱删
    #[test]
    fn a_fresh_machine_learns_from_the_cloud_ledger() {
        let (address, fake) = start_fake_s3();
        let key = generate_key().unwrap();

        let first = workspace("ledger-a");
        std::fs::write(
            first.root().join("db/objects/0/1.log"),
            "{\"rev\":1}\n".as_bytes(),
        )
        .unwrap();
        let settings = settings_for(&address, &key);
        run(&first, &settings, &|_| {}, false).unwrap();

        // 账本确实写到云端了
        assert!(
            fake.bucket
                .lock()
                .unwrap()
                .keys()
                .any(|name| name.ends_with(LATEST_KEY)),
            "跑完一趟应当留下账本"
        );

        // 甲机器再同步一次：账本在，什么都不用动
        let report = run(&first, &settings, &|_| {}, false).unwrap();
        assert_eq!(
            report.uploaded + report.downloaded,
            0,
            "账本在手，第二次该无事可做：{report:?}"
        );

        // 乙机器：把甲那份仓库整个抄过来，**唯独不带本地账本**（等于新装的机器）
        let second = workspace("ledger-b");
        std::fs::copy(
            first.root().join("db/objects/0/1.log"),
            second.root().join("db/objects/0/1.log"),
        )
        .unwrap();
        assert!(!second.settings_file("sync-index.json").exists());

        let report = run(&second, &settings, &|_| {}, false).unwrap();
        assert_eq!(
            report.uploaded + report.downloaded + report.removed_local,
            0,
            "照云端的账认路之后：不重传、不下载、更不删：{report:?}"
        );

        let _ = std::fs::remove_dir_all(first.root());
        let _ = std::fs::remove_dir_all(second.root());
    }

    /// 云端那把锁还热着：普通同步让你等着，**强制同步**现在就抢过来。
    ///
    /// 对应界面上那颗「强制同步」—— 另一边崩在半路时，不必干等它超时。
    #[test]
    fn a_forced_sync_takes_a_lock_that_is_still_warm() {
        let (address, bucket) = start_fake_s3();
        let settings = settings_for(&address, "");
        let s3 = S3::new(settings.s3.clone()).unwrap();
        let key = s3.config().key_of(LOCK_KEY);

        // 另一台机器刚拿的锁：时间戳是现在，离过期还早
        let held = serde_json::to_vec(&LockBody {
            host: "另一台机器".to_string(),
            at: OffsetDateTime::now_utc().unix_timestamp(),
        })
        .unwrap();
        bucket
            .bucket
            .lock()
            .unwrap()
            .insert(key.clone(), (held, "held".to_string()));

        // 普通同步：不动手，并且说清是锁挡着
        let polite = match acquire(&s3, false) {
            Ok(_) => panic!("锁还热着的时候不该动手"),
            Err(message) => message,
        };
        assert!(polite.contains("锁"), "该说清是锁挡着：{polite}");
        assert!(bucket.bucket.lock().unwrap().contains_key(&key), "锁还该在原处");

        // 强制：抢过来，锁里换成我们的名字
        let lock = acquire(&s3, true).expect("强制同步应当拿到锁");
        let holder: LockBody = serde_json::from_slice(&bucket.bucket.lock().unwrap()[&key].0).unwrap();
        assert_ne!(holder.host, "另一台机器", "锁该换成我们拿着了");

        // 放掉之后云端不留锁，下一次谁都能同步
        lock.release(&s3).unwrap();
        assert!(!bucket.bucket.lock().unwrap().contains_key(&key));
    }
}
