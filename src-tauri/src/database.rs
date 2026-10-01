//! 数据库：`~/.refind-note/db/` 的布局、元数据与识别。
//!
//! ```text
//! db/
//!   meta.json      认领标记、版本、建库时间
//!   config.json    数据语义设置（目前是封装策略）
//!   titles.json    id → 标题
//!   blobs/ab/<address>
//!   objects/0/<id>.log
//!   drafts/<id>
//!   trash/
//! ```
//!
//! 目录与那些 JSON 在打开时按需建出来：第一次启动就该是完整的，不必等写了一次才出现。

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::codec::Policy;
use crate::notes::Titles;
use crate::store::BlobStore;
use crate::workspace::write_json;

/// 认领标记：`meta.json` 里是它，才认这是重逢笔记的数据库
pub const KIND: &str = "refind-note";

/// 数据模型版本。**改数据格式就要动它**：不兼容的改动升主版本号。
pub const MODEL_VERSION: &str = "1.0.0";

/// 主命名空间的标识（也是它目录的名字）。
pub const MAIN_NS: &str = "0";

const META_FILE: &str = "meta.json";
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
}

impl Default for Config {
    fn default() -> Self {
        Self {
            // 压缩默认开着：正文多半是文本，压一下几乎总是划算，而且完全无感
            protection: Policy {
                compress: true,
                ..Default::default()
            },
        }
    }
}

#[derive(Debug)]
pub struct Database {
    root: PathBuf,
    meta: Meta,
}

impl Database {
    /// 打开（必要时建立）数据库，并认一遍它是谁的、什么版本。
    pub fn open(root: PathBuf) -> Result<Self, String> {
        for dir in [root.clone(), root.join(BLOBS_DIR), root.join(DRAFTS_DIR)] {
            fs::create_dir_all(&dir)
                .map_err(|error| format!("建不出目录 {}：{error}", dir.display()))?;
        }

        let meta = open_meta(&root)?;
        ensure_json(&root.join(TITLES_FILE), &Titles::default())?;
        ensure_json(&root.join(CONFIG_FILE), &Config::default())?;

        let database = Self { root, meta };
        database.ensure_namespace_dirs()?;
        Ok(database)
    }

    pub fn meta(&self) -> &Meta {
        &self.meta
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// 给主命名空间把两个目录备好：日志一处、已删除日志一处。
    ///
    /// 目录按命名空间分（`objects/<标识>/`、`trash/<标识>/`）——现在只有主命名空间。
    /// 每次打开都做一遍：目录不在的话，往那里写第一篇时会失败在"路径不存在"上。
    pub fn ensure_namespace_dirs(&self) -> Result<(), String> {
        for dir in [
            self.root.join(OBJECTS_DIR).join(MAIN_NS),
            self.root.join(TRASH_DIR).join(MAIN_NS),
        ] {
            fs::create_dir_all(&dir)
                .map_err(|error| format!("建不出目录 {}：{error}", dir.display()))?;
        }
        Ok(())
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
        self.root
            .join(TRASH_DIR)
            .join(ns)
            .join(format!("{id}.log"))
    }

    pub fn blobs(&self) -> BlobStore {
        BlobStore::new(self.root.join(BLOBS_DIR))
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
        crate::workspace::read_json(&self.root.join(CONFIG_FILE))
    }

    pub fn save_config(&self, config: &Config) -> Result<(), String> {
        write_json(&self.root.join(CONFIG_FILE), config)
    }

    /// 仓库默认的保护策略：新笔记从它出发，之后照每篇自己的最新一版
    pub fn protection(&self) -> Policy {
        self.config().protection
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

    #[test]
    fn a_fresh_directory_becomes_a_database() {
        let root = scratch("fresh");
        let database = Database::open(root.clone()).unwrap();

        assert_eq!(database.meta().kind, KIND);
        assert_eq!(database.meta().version, MODEL_VERSION);
        OffsetDateTime::parse(&database.meta().created_at, &Rfc3339).expect("该是 RFC3339");

        // 目录骨架与那几张表第一次打开就该在
        assert!(database.blobs().path_of("x").parent().unwrap().parent().unwrap().is_dir());
        assert!(database.drafts_dir().is_dir());
        assert!(database.titles_path().is_file());
        // 压缩默认开着：正文多半是文本，压一下几乎总是划算，而且无感
        let default_protection = database.config().protection;
        assert!(default_protection.compress);
        assert!(default_protection.gpg_sign.is_none());
        assert!(default_protection.gpg_encrypt.is_none());
        assert!(!default_protection.symmetric);

        // 重新打开认得出来，且不新建
        let again = Database::open(root.clone()).unwrap();
        assert_eq!(again.meta().created_at, database.meta().created_at);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_foreign_directory_is_refused() {
        let root = scratch("foreign");
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join(META_FILE),
            r#"{"kind":"别的东西","version":"1.0.0","created_at":"2026-01-01T00:00:00Z"}"#,
        )
        .unwrap();

        let error = Database::open(root.clone()).unwrap_err();
        assert!(error.contains("不是重逢笔记的数据库"), "{error}");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_version_mismatch_is_refused() {
        let root = scratch("version");
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join(META_FILE),
            r#"{"kind":"refind-note","version":"9.9.9","created_at":"2026-01-01T00:00:00Z"}"#,
        )
        .unwrap();

        let error = Database::open(root.clone()).unwrap_err();
        assert!(error.contains("9.9.9") && error.contains(MODEL_VERSION), "{error}");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn broken_metadata_is_refused_instead_of_being_overwritten() {
        let root = scratch("broken");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join(META_FILE), "{ 这不是 JSON").unwrap();

        assert!(Database::open(root.clone()).is_err());
        // 没有被当成"新建"而覆盖掉 —— 内容还在，人还能去看
        assert!(fs::read_to_string(root.join(META_FILE))
            .unwrap()
            .contains("这不是 JSON"));

        let _ = fs::remove_dir_all(&root);
    }
}
