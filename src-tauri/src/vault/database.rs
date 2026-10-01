//! 数据库：`~/.refind-note/db/` 的布局、元数据与识别。
//!
//! ```text
//! db/
//!   meta.json      认领标记、版本、建库时间
//!   titles.json    id → 显示标题
//!   namespaces.json  命名空间表（标题前缀那张表）
//!   blobs/ab/<address>
//!   objects/<命名空间>/<id>.log
//!   drafts/<id>
//!   trash/<命名空间>/<id>.log
//!
//! settings/
//!   config.json    数据语义设置（目前是封装策略）—— 跟着仓库走
//! ```
//!
//! 目录与那些 JSON 在打开时按需建出来：第一次启动就该是完整的，不必等写了一次才出现。

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::storage::codec::Policy;
use crate::storage::store::BlobStore;
use crate::storage::workspace::{write_json, Workspace};
use crate::vault::namespace::{NamespaceTable, MAIN_ID};
use crate::vault::notes::Titles;

/// 认领标记：`meta.json` 里是它，才认这是重逢笔记的数据库
pub const KIND: &str = "refind-note";

/// 数据模型版本。**改数据格式就要动它**：不兼容的改动升主版本号。
pub const MODEL_VERSION: &str = "1.0.0";

const META_FILE: &str = "meta.json";
const NAMESPACES_FILE: &str = "namespaces.json";
const CONFIG_FILE: &str = "config.json";
const TITLES_FILE: &str = "titles.json";
const BLOBS_DIR: &str = "blobs";
const OBJECTS_DIR: &str = "objects";
const DRAFTS_DIR: &str = "drafts";
const TRASH_DIR: &str = "trash";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Meta {
    /// 认领标记（见 [`KIND`]）
    pub kind: String,
    /// 数据模型版本（见 [`MODEL_VERSION`]）
    pub version: String,
    /// 建库时间（RFC3339，UTC）
    pub created_at: String,
}

/// 数据语义设置。外观那些是"这台机器"的事，在 `settings/` 下，不在这里。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// 默认的封装策略：还没有正文的新笔记从它出发
    pub protection: Policy,
    /// 回收站里的条目留多少天（到期由自动维护清掉）
    pub trash_keep_days: u64,
    /// 自动整理的间隔：隔这么多天跑一次
    pub gc_interval_days: u64,
    /// 上次清回收站的时间（RFC3339）；空串 = 从没做过
    pub last_trash_purge: String,
    /// 上次整理的时间（RFC3339）；空串 = 从没做过
    pub last_gc: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            // 压缩默认开着：正文多半是文本，压一下几乎总是划算，而且完全无感
            protection: Policy {
                compress: true,
                ..Default::default()
            },
            // 回收站留一个月：够长到"删错了还来得及捞"，也够短到"不会攒成垃圾场"
            trash_keep_days: 30,
            // 整理**每天一次**：它只是扫一遍日志与内容块，开销小，也不动还有引用的东西
            gc_interval_days: 1,
            last_trash_purge: String::new(),
            last_gc: String::new(),
        }
    }
}

#[derive(Debug)]
pub struct Database {
    root: PathBuf,
    /// 数据语义设置（`config.json`）所在的那一层
    settings: PathBuf,
    meta: Meta,
}

impl Database {
    /// 打开（必要时建立）数据库，并认一遍它是谁的、什么版本。
    ///
    /// 数据库自己的文件都在 `db/` 下；**数据语义设置**（`config.json`）放到
    /// 工作目录的 `settings/` 下 —— 它跟着仓库走，但和别的配置文件放在一处。
    pub fn open(workspace: &Workspace) -> Result<Self, String> {
        let root = workspace.database_dir();
        let settings = workspace.settings_dir();

        for dir in [root.clone(), root.join(BLOBS_DIR), root.join(DRAFTS_DIR)] {
            fs::create_dir_all(&dir)
                .map_err(|error| format!("建不出目录 {}：{error}", dir.display()))?;
        }

        let meta = open_meta(&root)?;
        ensure_json(&root.join(TITLES_FILE), &Titles::default())?;
        // 命名空间表放在 db/ 而不是 settings/：它决定"一个标题说的是哪一篇"，
        // 是这批数据的一部分（换台机器读同一份仓库，也得认出同一批标题）
        ensure_json(&root.join(NAMESPACES_FILE), &NamespaceTable::default())?;
        ensure_json(&settings.join(CONFIG_FILE), &Config::default())?;

        let database = Self {
            root,
            settings,
            meta,
        };

        // 默认的跨站命名空间只在新仓库上播一次（见 `sow_defaults`）
        let mut table = database.namespaces();
        if table.sow_defaults() {
            database.save_namespaces(&table)?;
        }

        database.ensure_namespace_dirs()?;
        Ok(database)
    }

    /// 命名空间表（标题前缀那张表）
    pub fn namespaces(&self) -> NamespaceTable {
        crate::storage::workspace::read_json(&self.root.join(NAMESPACES_FILE))
    }

    pub fn save_namespaces(&self, table: &NamespaceTable) -> Result<(), String> {
        write_json(&self.root.join(NAMESPACES_FILE), table)
    }

    pub fn meta(&self) -> &Meta {
        &self.meta
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// 给每个**可存储**的命名空间把两个目录备好：日志一处、已删除日志一处。
    ///
    /// 目录按命名空间分（`objects/<标识>/`、`trash/<标识>/`）。
    /// 每次打开都做一遍：目录不在的话，往那里写第一篇时会失败在"路径不存在"上。
    pub fn ensure_namespace_dirs(&self) -> Result<(), String> {
        for ns in self.namespaces().storable_ids() {
            self.ensure_namespace_dir(&ns)?;
        }
        Ok(())
    }

    /// 给某一个命名空间把两个目录备好（新建命名空间时补建）
    pub fn ensure_namespace_dir(&self, ns: &str) -> Result<(), String> {
        for dir in [
            self.root.join(OBJECTS_DIR).join(ns),
            self.root.join(TRASH_DIR).join(ns),
        ] {
            fs::create_dir_all(&dir)
                .map_err(|error| format!("建不出目录 {}：{error}", dir.display()))?;
        }
        Ok(())
    }

    /// 表里所有命名空间的标识（备份、遍历用）
    pub fn namespace_ids(&self) -> Vec<String> {
        self.namespaces()
            .items
            .iter()
            .map(|item| item.id.clone())
            .collect()
    }

    /// 某个命名空间下的笔记日志
    pub fn note_path(&self, ns: &str, id: &str) -> PathBuf {
        self.root
            .join(OBJECTS_DIR)
            .join(ns)
            .join(format!("{id}.log"))
    }

    /// 某个命名空间下已删除笔记的日志
    pub fn trash_note_path(&self, ns: &str, id: &str) -> PathBuf {
        self.root.join(TRASH_DIR).join(ns).join(format!("{id}.log"))
    }

    /// 主命名空间的标识（其他地方要它时从这里拿）
    pub fn main_ns(&self) -> &'static str {
        MAIN_ID
    }

    pub fn blobs(&self) -> BlobStore {
        BlobStore::new(self.root.join(BLOBS_DIR))
    }

    /// 配置所在的那一层（`settings/`）：偏好、语义设置、浏览历史都在这儿
    pub fn settings_dir(&self) -> &Path {
        &self.settings
    }

    /// 笔记日志所在的目录（整理那一轮要扫它）
    pub fn objects_dir(&self) -> PathBuf {
        self.root.join(OBJECTS_DIR)
    }

    /// 已删除笔记的日志目录
    pub fn trash_dir(&self) -> PathBuf {
        self.root.join(TRASH_DIR)
    }

    pub fn drafts_dir(&self) -> PathBuf {
        self.root.join(DRAFTS_DIR)
    }

    pub fn draft_path(&self, id: &str) -> PathBuf {
        self.drafts_dir().join(id)
    }

    pub fn titles_path(&self) -> PathBuf {
        self.root.join(TITLES_FILE)
    }

    pub fn config(&self) -> Config {
        crate::storage::workspace::read_json(&self.settings.join(CONFIG_FILE))
    }

    pub fn save_config(&self, config: &Config) -> Result<(), String> {
        write_json(&self.settings.join(CONFIG_FILE), config)
    }

    /// 仓库默认的保护策略：新笔记从它出发，之后照每篇自己的最新一版
    pub fn protection(&self) -> Policy {
        self.config().protection
    }

    /// 这台计算机上有没有可用的 gpg。
    ///
    /// 没有的话，签名 / 加密这类功能不可用：用到它们时（提交、草稿、读 gpg 封装过的
    /// 内容）会得到一句说得清的错；其余功能照常。调用方可以据此提前把这些选项收起来。
    pub fn gpg_available(&self) -> bool {
        crate::storage::codec::gpg_available()
    }
}

/// 读元数据；目录里没有就当新建，落一份进去。
fn open_meta(root: &Path) -> Result<Meta, String> {
    let path = root.join(META_FILE);

    if !path.exists() {
        let meta = Meta {
            kind: KIND.to_string(),
            version: MODEL_VERSION.to_string(),
            created_at: now(),
        };
        write_json(&path, &meta)?;
        return Ok(meta);
    }

    let text =
        fs::read_to_string(&path).map_err(|error| format!("读不出 {}：{error}", path.display()))?;
    let meta: Meta = serde_json::from_str(&text)
        .map_err(|error| format!("{} 不成形：{error}", path.display()))?;

    identify(&meta)?;
    Ok(meta)
}

/// 认一遍：是不是我们的库、版本对不对。
pub fn identify(meta: &Meta) -> Result<(), String> {
    if meta.kind != KIND {
        return Err(format!(
            "这个目录不是重逢笔记的数据库（标记是「{}」）",
            meta.kind
        ));
    }

    if meta.version != MODEL_VERSION {
        return Err(format!(
            "数据库版本是 {}，这个程序认得的是 {MODEL_VERSION}",
            meta.version
        ));
    }

    Ok(())
}

/// 表文件不在就落一份默认的：该在的东西第一次打开就该在。
fn ensure_json<T: Serialize + Default>(path: &Path, default: &T) -> Result<(), String> {
    if path.exists() {
        return Ok(());
    }
    write_json(path, default)
}

/// 当前时间，RFC3339（UTC）。格式化几乎不会失败，兜底给空串。
pub fn now() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "refind-note-database-test-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    /// 从暂存目录开一个库（工作目录的骨架先建出来）
    fn open(dir: &Path) -> Result<Database, String> {
        let workspace = Workspace::open(dir.to_path_buf())?;
        Database::open(&workspace)
    }

    #[test]
    fn a_fresh_directory_becomes_a_database() {
        let root = scratch("fresh");
        let database = open(&root).unwrap();

        assert_eq!(database.meta().kind, KIND);
        assert_eq!(database.meta().version, MODEL_VERSION);
        OffsetDateTime::parse(&database.meta().created_at, &Rfc3339).expect("该是 RFC3339");

        // 目录骨架与那几张表第一次打开就该在
        assert!(database
            .blobs()
            .path_of("x")
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .is_dir());
        assert!(database.drafts_dir().is_dir());
        assert!(database.titles_path().is_file());
        // 压缩默认开着：正文多半是文本，压一下几乎总是划算，而且无感
        let default_protection = database.config().protection;
        assert!(default_protection.compress);
        assert!(default_protection.gpg_sign.is_none());
        assert!(default_protection.gpg_encrypt.is_none());
        assert!(!default_protection.symmetric);

        // 重新打开认得出来，且不新建
        let again = open(&root).unwrap();
        assert_eq!(again.meta().created_at, database.meta().created_at);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_foreign_directory_is_refused() {
        let root = scratch("foreign");
        let database_dir = root.join("db");
        fs::create_dir_all(&database_dir).unwrap();
        fs::write(
            database_dir.join(META_FILE),
            r#"{"kind":"别的东西","version":"1.0.0","created_at":"2026-01-01T00:00:00Z"}"#,
        )
        .unwrap();

        let error = open(&root).unwrap_err();
        assert!(error.contains("不是重逢笔记的数据库"), "{error}");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_version_mismatch_is_refused() {
        let root = scratch("version");
        let database_dir = root.join("db");
        fs::create_dir_all(&database_dir).unwrap();
        fs::write(
            database_dir.join(META_FILE),
            r#"{"kind":"refind-note","version":"9.9.9","created_at":"2026-01-01T00:00:00Z"}"#,
        )
        .unwrap();

        let error = open(&root).unwrap_err();
        assert!(
            error.contains("9.9.9") && error.contains(MODEL_VERSION),
            "{error}"
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn broken_metadata_is_refused_instead_of_being_overwritten() {
        let root = scratch("broken");
        let database_dir = root.join("db");
        fs::create_dir_all(&database_dir).unwrap();
        fs::write(database_dir.join(META_FILE), "{ 这不是 JSON").unwrap();

        assert!(open(&root).is_err());
        // 没有被当成"新建"而覆盖掉 —— 内容还在，人还能去看
        assert!(fs::read_to_string(database_dir.join(META_FILE))
            .unwrap()
            .contains("这不是 JSON"));

        let _ = fs::remove_dir_all(&root);
    }
}
