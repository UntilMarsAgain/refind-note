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

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::storage::s3::{S3Config, S3};
use crate::storage::workspace::{read_json, write_json, Workspace};

/// 锁多久没动静算过期（秒）
const LOCK_TTL: i64 = 300;

/// 锁放在云端哪个键上（在配置的前缀之下）
const LOCK_KEY: &str = ".sync-lock.json";

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

    /// 这一份设置里那把钥匙（认得出才给）
    pub fn cipher(&self) -> Result<CloudCipher, String> {
        CloudCipher::from_key(&self.key)
    }
}

/// 生成一把新的云端钥匙：32 字节随机，写成 base64 给人抄
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
            // 用仓库默认那一档（与笔记那边一致：选了国密就都是国密）
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
    size: u64,
    /// 本机文件的时间（Unix 秒）
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
    size: u64,
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
    local: Option<&Local>,
    remote: Option<&Remote>,
    aligned: Option<&Stamp>,
) -> (Decision, String) {
    match (local, remote, aligned) {
        // ---- 两边都有 ----
        (Some(local), Some(remote), aligned) => {
            // "变没变"看两样：本机看（大小、时间），云端看 ETag
            let local_changed = aligned
                .map(|stamp| stamp.size != local.size || stamp.mtime != local.mtime)
                .unwrap_or(true);
            let remote_changed = aligned
                .map(|stamp| stamp.etag != remote.etag)
                .unwrap_or(true);

            match (local_changed, remote_changed) {
                (false, false) => (Decision::Nothing, String::new()),
                (true, false) => (Decision::Upload, String::new()),
                (false, true) => (Decision::Download, String::new()),
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
            // 上次对齐时云端有它、现在没了 —— 那是别的机器删了。
            // 但**只有本机这一份没动过**才跟着删：改过的那些是新的，
            // 传回去（宁可多一份，也别把刚写的东西删掉）
            Some(stamp) if stamp.size == local.size && stamp.mtime == local.mtime => {
                (Decision::DeleteLocal, "云端已经删掉".to_string())
            }
            Some(_) => (Decision::Upload, "云端删过，但本机这份改过".to_string()),
            // 从来没有过：新写的，传上去
            None => (Decision::Upload, String::new()),
        },

        // ---- 只有云端有 ----
        (None, Some(_), aligned) => match aligned {
            // 上次对齐时本机是有它的 —— 那是本机删了，云端跟着删
            Some(_) => (Decision::DeleteRemote, "本机已经删掉".to_string()),
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
        text: "正在取得云端同步锁…".to_string(),
    });
    let lock = acquire(&s3)?;

    let outcome = reconcile(&s3, &root, &mut index, transform.as_ref(), progress);

    // 不管成没成，锁都要放掉：留着它，别的机器要等过期才能动
    let _ = lock.release(&s3);

    let report = outcome?;
    index.save(workspace)?;
    Ok(report)
}

/// 对账 + 动手（锁已经拿到了）
fn reconcile(
    s3: &S3,
    root: &Path,
    index: &mut Index,
    transform: &dyn SyncTransform,
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

    // 决定怎么办（纯规则）
    let planned: Vec<(String, Decision, String)> = paths
        .iter()
        .map(|path| {
            let (decision, why) = decide(local.get(path), remote.get(path), index.files.get(path));
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
        let text = describe(path, decision);
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
                stamp_local(index, root, path, fetched.etag);
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

fn describe(path: &str, decision: &Decision) -> String {
    match decision {
        Decision::Upload | Decision::TakeNewer("local") => format!("上传 {path}"),
        Decision::Download | Decision::TakeNewer("remote") => format!("下载 {path}"),
        Decision::DeleteRemote => format!("云端删除 {path}"),
        Decision::DeleteLocal => format!("本机删除 {path}"),
        _ => path.to_string(),
    }
}

/// 记下这一份现在的样子（大小、时间、云端的 ETag）
fn stamp_local(index: &mut Index, root: &Path, path: &str, etag: String) {
    let Ok(meta) = fs::metadata(root.join(path)) else {
        return;
    };
    index.files.insert(
        path.to_string(),
        Stamp {
            size: meta.len(),
            mtime: mtime_of(&meta),
            etag,
        },
    );
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
        out.insert(
            relative,
            Local {
                size: meta.len(),
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
    // 锁是云端的东西，不是仓库里的文件 —— 别把它下载到工作目录里
    if relative.is_empty() || relative == LOCK_KEY || !is_synced(relative) {
        return None;
    }
    Some(relative.to_string())
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
fn acquire(s3: &S3) -> Result<Lock, String> {
    let key = s3.config().key_of(LOCK_KEY);
    let host = hostname();
    let now = OffsetDateTime::now_utc().unix_timestamp();
    let body = serde_json::to_vec(&LockBody { host, at: now })
        .map_err(|error| format!("锁序列化失败：{error}"))?;

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
                    "云端同步锁正被「{who}」拿着，过一会儿再试（或者等它超时）"
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
    std::env::var("HOSTNAME")
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

    fn local(size: u64, mtime: i64) -> Local {
        Local { size, mtime }
    }

    fn remote(etag: &str, modified: i64) -> Remote {
        Remote {
            size: 10,
            etag: etag.to_string(),
            modified,
        }
    }

    fn stamp(size: u64, mtime: i64, etag: &str) -> Stamp {
        Stamp {
            size,
            mtime,
            etag: etag.to_string(),
        }
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
            Some(&local(10, 100)),
            Some(&remote("aaa", 100)),
            Some(&stamp(10, 100, "aaa")),
        );
        assert_eq!(decision, Decision::Nothing);
    }

    #[test]
    fn only_the_local_side_changed_uploads() {
        let (decision, _) = decide(
            Some(&local(20, 200)),
            Some(&remote("aaa", 100)),
            Some(&stamp(10, 100, "aaa")),
        );
        assert_eq!(decision, Decision::Upload);
    }

    #[test]
    fn only_the_remote_side_changed_downloads() {
        let (decision, _) = decide(
            Some(&local(10, 100)),
            Some(&remote("bbb", 200)),
            Some(&stamp(10, 100, "aaa")),
        );
        assert_eq!(decision, Decision::Download);
    }

    /// 两边都动过：云端更新就取云端，本机不旧就留本机（**不静默丢东西**，会记一条）
    #[test]
    fn a_real_conflict_is_settled_by_time() {
        let (decision, why) = decide(
            Some(&local(20, 200)),
            Some(&remote("bbb", 300)),
            Some(&stamp(10, 100, "aaa")),
        );
        assert_eq!(decision, Decision::TakeNewer("remote"));
        assert!(!why.is_empty(), "取哪边、为什么，要说得出来");

        let (decision, _) = decide(
            Some(&local(20, 400)),
            Some(&remote("bbb", 300)),
            Some(&stamp(10, 100, "aaa")),
        );
        assert_eq!(decision, Decision::TakeNewer("local"));

        // 时间几乎一样时保守：留本机那份（宁可多传一次，也别把刚写的盖掉）
        let (decision, _) = decide(
            Some(&local(20, 300)),
            Some(&remote("bbb", 300)),
            Some(&stamp(10, 100, "aaa")),
        );
        assert_eq!(decision, Decision::TakeNewer("local"));
    }

    /// 新写的传上去，云端多出来的拿回来
    #[test]
    fn a_first_sync_takes_the_union() {
        let (decision, _) = decide(Some(&local(10, 100)), None, None);
        assert_eq!(decision, Decision::Upload);

        let (decision, _) = decide(None, Some(&remote("aaa", 100)), None);
        assert_eq!(decision, Decision::Download);
    }

    /// 删除要**两边都认**才动：一边删了、另一边没改，才跟着删
    #[test]
    fn deletions_travel_only_when_the_other_side_is_untouched() {
        // 本机删了（索引里有、本机没有），云端还在 → 云端也删
        let (decision, why) = decide(
            None,
            Some(&remote("aaa", 100)),
            Some(&stamp(10, 100, "aaa")),
        );
        assert_eq!(decision, Decision::DeleteRemote);
        assert!(!why.is_empty());

        // 云端删了，本机没动过 → 本机也删
        let (decision, _) = decide(Some(&local(10, 100)), None, Some(&stamp(10, 100, "aaa")));
        assert_eq!(decision, Decision::DeleteLocal);

        // 云端删了、本机**改过** → 不跟着删（本机那份是新的，传上去）
        let (decision, _) = decide(Some(&local(20, 300)), None, Some(&stamp(10, 100, "aaa")));
        assert_eq!(
            decision,
            Decision::Upload,
            "本机改过的那份不能被云端的删除带走"
        );
    }
}
