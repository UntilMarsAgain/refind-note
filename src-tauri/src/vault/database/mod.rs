//   Refind Note is a note-taking software.
//   Copyright (C) 2026 Until Mars Again
//
//   This program is free software: you can redistribute it and/or modify
//   it under the terms of the GNU Affero General Public License as published by
//   the Free Software Foundation, either version 3 of the License, or
//   (at your option) any later version.
//
//   This program is distributed in the hope that it will be useful,
//   but WITHOUT ANY WARRANTY; without even the implied warranty of
//   MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
//   GNU Affero General Public License for more details.
//
//   You should have received a copy of the GNU Affero General Public License
//   along with this program.  If not, see <http://www.gnu.org/licenses/>.

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
//!   repository.json  这个仓库自己的设置（默认封装策略、回收站保留、整理间隔）
//! ```
//!
//! 目录与那些 JSON 在打开时按需建出来：第一次启动就该是完整的，不必等写了一次才出现。
//!
//! ## 这一层怎么分的
//!
//! | 文件 | 管什么 |
//! |---|---|
//! | 本文件 | 库布局、`Meta`/`Config`、认领标记，以及打开库时把目录骨架铺出来 |
//! | [`facts`] | 仓库里有多少东西 —— 诊断页与设置页要的那些数字 |
//!
//! `Database` 是这一层的**能力句柄**：笔记、命名空间、草稿那些方法都挂在它身上，
//! 各自住在 [`crate::vault::notes`] / [`crate::vault::namespace`] 里。

mod facts;
#[cfg(test)]
mod tests;

pub use facts::RepositoryFacts;

use facts::{count_files, directory_bytes};

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::storage::codec::Policy;
use crate::storage::store::BlobStore;
use crate::storage::workspace::{write_json, Workspace};
use crate::vault::namespace::{NamespaceTable, MAIN_ID};

pub const KIND: &str = "refind-note";

/// 数据模型版本。**改数据格式就要动它**：不兼容的改动升主版本号。
pub const MODEL_VERSION: &str = "1.0.0";

const META_FILE: &str = "meta.json";
const NAMESPACES_FILE: &str = "namespaces.json";
const CONFIG_FILE: &str = "repository.json";
/// 老仓库里这个文件叫 `config.json`：名字看不出它是仓库的，改名时留一条迁移
const LEGACY_CONFIG_FILE: &str = "config.json";
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
    /// 仓库设置（`repository.json`）所在的那一层
    settings: PathBuf,
    meta: Meta,
}

impl Database {
    /// 打开（必要时建立）数据库，并认一遍它是谁的、什么版本。
    ///
    /// 数据库自己的文件都在 `db/` 下；**仓库自己的设置**（`repository.json`，
    /// 默认封装策略、回收站保留天数、整理间隔）放到工作目录的 `settings/` 下 ——
    /// 它跟着这份仓库走（换台机器读同一份仓库，行为要一样），只是和别的配置文件放在一处。
    /// 名字里写的是 **repository**：`settings/` 下还有 `preferences.json`（这台机器的偏好），
    /// 不点明的话，从文件名看不出哪个跟仓库走、哪个跟机器走。
    pub fn open(workspace: &Workspace) -> Result<Self, String> {
        let root = workspace.database_dir();
        let settings = workspace.settings_dir();

        for dir in [root.clone(), root.join(BLOBS_DIR), root.join(DRAFTS_DIR)] {
            fs::create_dir_all(&dir)
                .map_err(|error| format!("建不出目录 {}：{error}", dir.display()))?;
        }

        let meta = open_meta(&root)?;
        // **空表不写**：写下来就成了"本机改过"，同步时会跟云端那份打架 ——
        // 新机器上前两样本来就没有，正好让云端那份落下来。
        // 读的时候没有文件就是默认值（见 `read_json`），用到才写。
        //
        // 老仓库里它叫 `config.json`：上一次改名之前写下的，先搬过来，
        // 免得"换个文件名"就把设置读丢、退回默认值
        let legacy = settings.join(LEGACY_CONFIG_FILE);
        let current = settings.join(CONFIG_FILE);
        if legacy.is_file() && !current.is_file() {
            fs::rename(&legacy, &current)
                .map_err(|error| format!("搬不动 {}：{error}", legacy.display()))?;
        }

        let database = Self {
            root,
            settings,
            meta,
        };

        database.ensure_namespace_dirs()?;
        Ok(database)
    }

    /// 命名空间表（标题前缀那张表）。
    ///
    /// 读的时候顺手做两件修补，但**不写盘**：缺的内建补上（老仓库可能还没有
    /// `File:` / `Help:`），表文件压根不存在时把默认的跨站命名空间摆上。
    /// 写盘留给下一次真正的改动 —— 这台上没动过的东西，磁盘上就不该凭空多一个文件
    /// （那会被同步当成"本机改过"，跟云端那份打架）。
    pub fn namespaces(&self) -> NamespaceTable {
        let mut table: NamespaceTable =
            crate::storage::workspace::read_json(&self.root.join(NAMESPACES_FILE));
        let fresh = !self.root.join(NAMESPACES_FILE).is_file();
        table.ensure_builtins();
        if fresh {
            table.sow_defaults();
        }
        table
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
    /// 仓库里有多少东西 —— 只数文件名与大小，**不读内容**（诊断页要报）。
    ///
    /// 刻意不去数"有多少篇笔记"：那要把每篇的事件日志读一遍，加密的那些还会碰锁。
    /// 这里报的是磁盘上的实情：日志几份、内容块几个、占多少、草稿与回收站各几条。
    pub fn facts(&self) -> RepositoryFacts {
        // `root` 就是 `db/` 那一层（blobs / objects / drafts 都在它下面）
        let (blobs, blob_bytes) = count_files(&self.root.join(BLOBS_DIR));
        RepositoryFacts {
            logs: count_files(&self.objects_dir()).0,
            drafts: count_files(&self.drafts_dir()).0,
            trash: count_files(&self.trash_dir()).0,
            blobs,
            blob_bytes,
            database_bytes: directory_bytes(&self.root),
        }
    }

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

    /// 仓库设置。文件不在就是默认值 —— 与命名空间表同一个道理：**不主动写**，
    /// 写下来就会被同步当成"本机改过"。改了设置（`save_config`）才落盘。
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

/// 当前时间，RFC3339（UTC）。格式化几乎不会失败，兜底给空串。
pub fn now() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_default()
}
