//! **账本**：同步自己的设置（只在这台机器上）与"上次对齐时每一份是什么样"。
//!
//! 这里放两份小 JSON，都**只存在于本地**（`is_synced` 不认它们，所以永远不会被传上去）：
//!
//! - `settings/sync.json` —— 设置与那把云端钥匙（[`SyncSettings`]）；
//! - `settings/sync-index.json` —— 对齐索引（[`Index`]）：每个文件记一份"上次对齐时的样子"。
//!
//! 云端还留着一份**同格式**的账本（键见 `LATEST_KEY`，由 `mod.rs` 读写）：本地这份答不了
//! "这台机器是不是新装的"与"清单里没有的那一份到底是不是被删了"，云端那份能。
//!
//! 给界面看的那一份在 [`SyncSettingsView`]：它**不含任何秘密** —— 密钥与 S3 私钥只报
//! "有没有"。界面改完交回来的那份是 [`SyncSettingsPatch`]，其中私钥留空即"不改"。

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::storage::s3::S3Config;
use crate::storage::workspace::{read_json, write_json, Workspace};

use super::cipher::CloudCipher;

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

/// 同步的设置文件（`settings/sync.json`）
fn settings_path(workspace: &Workspace) -> PathBuf {
    workspace.settings_file("sync.json")
}

/// 对齐索引（`settings/sync-index.json`）
pub(super) fn index_path(workspace: &Workspace) -> PathBuf {
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

/// 密钥只让本人读得动
fn restrict(path: &std::path::Path) {
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

/// 上次对齐时的样子
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct Stamp {
    /// 上次对齐时本机这一份的内容指纹
    pub(super) hash: String,
    /// 本机文件的时间（Unix 秒）—— 现在只用来给人看，判"变没变"看的是指纹
    #[serde(default)]
    pub(super) mtime: i64,
    /// 云端那一份的 ETag
    pub(super) etag: String,
}

/// 对齐索引：哪些文件已经对齐了（`settings/sync-index.json`）
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct Index {
    /// 相对路径 → 上次对齐时的样子
    pub(super) files: BTreeMap<String, Stamp>,
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