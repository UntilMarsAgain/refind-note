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

//! 工作目录：`~/.refind-note` 的位置、目录骨架与文件读写。
//!
//! 布局：
//!
//! ```text
//! ~/.refind-note/
//!   settings/          这台机器的偏好（preferences.json）
//!   db/                数据库
//! ```
//!
//! 目录在打开时按需建出来，用户第一次启动就能直接用，不必先手动 mkdir。
//!
//! 这一层还回答一件事：**哪些东西该跟着仓库走、哪些是这台机器的**（见
//! [`is_synced`]）—— 同步那边照这个答案办事，不必自己认路径。

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::de::DeserializeOwned;
use serde::Serialize;

/// 工作目录名（放在用户主目录下）
pub const DIR_NAME: &str = ".refind-note";

/// 装配层指定的仓库位置（移动端用）。
///
/// 桌面上有"用户主目录"这回事，`~/.refind-note` 在哪儿不用问别人；手机上不存在这个
/// 概念 —— 能写的地方是**程序自己的目录**（`/data/user/0/<包名>/files`，卸载才没），
/// 那个路径只有拿到 `AppHandle` 才知道，所以由启动时的那一层告诉这里。
static INSTALLED_ROOT: OnceLock<PathBuf> = OnceLock::new();

/// 把仓库的位置定下来（只认第一次；移动端在启动时叫一次）
pub fn install_root(root: PathBuf) {
    let _ = INSTALLED_ROOT.set(root);
}

/// 数据库文件夹
const DB_DIR: &str = "db";

/// 设置与浏览状态文件夹
const SETTINGS_DIR: &str = "settings";

/// 这一份**该不该同步到云端**。
///
/// 判据只有一条：**是不是这台机器自己的东西**。默认都同步 —— 工作目录里新长出来的
/// 东西因此自动跟上，不会因为"同步模块没认得它"而悄悄漏掉（那才是真的丢数据）。
/// 少数几样是本机专属的，它们在这里点名：
///
/// - `settings/preferences.json`：界面偏好（缩放、主题、标签栏）—— 换台机器本来就该重来；
/// - `settings/browsing.jsonl`：浏览历史；
/// - `settings/sync.json` 与 `settings/sync-index.json`：同步自己的设置与索引，
///   里面还有 S3 的密钥；
/// - `db/drafts/`：写了一半的草稿槽位（每篇一个、会被覆盖），本机的缓冲，不算内容；
/// - 任何 `.tmp`：那是落盘写到一半的名字（见 [`write_bytes`]），不该传。
///
/// 往后往工作目录里加东西的人：**要是不该同步，来这里加一行并说清为什么**；
/// 该同步的什么都不必做。
pub fn is_synced(relative: &str) -> bool {
    let relative = relative.trim_start_matches("./");
    if relative.is_empty() || relative.ends_with(".tmp") {
        return false;
    }
    !matches!(
        relative,
        "settings/preferences.json"
            | "settings/browsing.jsonl"
            | "settings/sync.json"
            | "settings/sync-index.json"
    ) && !relative.starts_with("db/drafts/")
}

pub struct Workspace {
    root: PathBuf,
}

impl Workspace {
    /// 打开指定的工作目录，必要时建出目录骨架
    pub fn open(root: PathBuf) -> Result<Self, String> {
        let workspace = Self { root };
        workspace.ensure()?;
        Ok(workspace)
    }

    /// 打开默认工作目录：桌面上是 `~/.refind-note`，移动端是程序自己的目录
    /// （见 [`install_root`]）
    pub fn open_default() -> Result<Self, String> {
        if let Some(installed) = INSTALLED_ROOT.get() {
            return Self::open(installed.clone());
        }
        Self::open(home_dir()?.join(DIR_NAME))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// 数据库文件夹
    pub fn database_dir(&self) -> PathBuf {
        self.root.join(DB_DIR)
    }

    /// 设置文件夹
    pub fn settings_dir(&self) -> PathBuf {
        self.root.join(SETTINGS_DIR)
    }

    /// 设置文件夹里的一个文件
    pub fn settings_file(&self, name: &str) -> PathBuf {
        self.settings_dir().join(name)
    }

    fn ensure(&self) -> Result<(), String> {
        for dir in [self.root.clone(), self.database_dir(), self.settings_dir()] {
            fs::create_dir_all(&dir)
                .map_err(|error| format!("建不出目录 {}：{error}", dir.display()))?;
        }
        Ok(())
    }
}

/// 用户主目录。
///
/// 三个系统三套说法：Linux/macOS 是 `HOME`，Windows 是 `USERPROFILE`（还有一套
/// 注册表里的"实际配置目录"，`dirs` 帮我们走那条最准的路）。环境变量都靠不住时
/// 它会去查系统，比我们自己挑环境变量可靠。
fn home_dir() -> Result<PathBuf, String> {
    dirs::home_dir().ok_or_else(|| "找不到用户主目录".to_string())
}

/// 读一个 JSON 文件；不在、读不动、或内容不成形，一律给默认值。
///
/// 这些文件用户看得见、也可能手改，所以这里不让程序起不来 —— 读不动就当没设置过。
pub fn read_json<T: DeserializeOwned + Default>(path: &Path) -> T {
    let Ok(text) = fs::read_to_string(path) else {
        return T::default();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

/// 原子写一个 JSON 文件：先写同目录的临时文件，再改名顶上去。
///
/// 直接往目标文件写的话，写到一半被读到就是半截 JSON；改名在同一个文件系统内是
/// 原子的，读到的要么是旧的完整内容，要么是新的。
pub fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let data = serde_json::to_vec_pretty(value).map_err(|error| format!("序列化失败：{error}"))?;
    write_bytes(path, &data)
}

/// 原子写字节。blob 与 JSON 表都走这一条 —— 落盘要么是整个新的，要么是整个旧的。
pub fn write_bytes(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("建不出目录 {}：{error}", parent.display()))?;
    }

    let temporary = path.with_extension("tmp");
    write_private(&temporary, bytes)
        .map_err(|error| format!("写入 {} 失败：{error}", temporary.display()))?;
    replace_file(&temporary, path)
        .map_err(|error| format!("落盘 {} 失败：{error}", path.display()))?;

    Ok(())
}

/// 写一个**只有本人读得动**的文件。
///
/// ## 为什么不是 `fs::write`
///
/// `fs::write` 建出来的文件走 umask，通常是 `0644` —— **别人读得动**。
/// 而这个仓库里落着笔记正文、内容块、以及 `settings/sync.json` 里的
/// **云端钥匙与 S3 私钥**。
///
/// 之前靠"写完再 `chmod`"（见 `features::sync::settings::restrict`）补，
/// 但那有个**窗口**：从落盘到改权限之间，文件已经是 world-readable 的了；
/// 而进程要是正好死在那个窗口里，那份文件就**永远**是 `0644`。
///
/// 所以权限在**创建那一刻**就给对（`OpenOptions::mode`），并且落笔之后再
/// 显式设一次 —— `mode` 只在**新建**时生效，崩溃残留的临时文件是旧的、不会改。
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;

    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    let mut file = options.open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // 见上：上一次崩溃留下的临时文件是旧的，`mode` 对它不生效
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    file.write_all(bytes)?;
    Ok(())
}

/// 把临时文件改名顶到目标上。
///
/// **Windows 上 rename 盖不住已经存在的文件**（Unix 可以）：那里得先把旧的删掉。
/// 于是那一小段不是原子的 —— 但只在 Windows 上、只在"目标已存在"时走到，而且断在
/// 中间最坏是这份文件没了（内容还在别处：同步过的在云端，没同步的本来也只有这一份）。
/// 比"从第二次写入起每个文件都写不进去"要好得多。
fn replace_file(temporary: &Path, target: &Path) -> std::io::Result<()> {
    match fs::rename(temporary, target) {
        Ok(()) => Ok(()),
        Err(_) if cfg!(windows) => {
            fs::remove_file(target)?;
            fs::rename(temporary, target)
        }
        Err(error) => Err(error),
    }
}

/// 追加一行（日志用）。只追加、不重写，崩溃最多丢最后一行。
pub fn append_line(path: &Path, line: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("建不出目录 {}：{error}", parent.display()))?;
    }

    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| format!("打不开 {}：{error}", path.display()))?;

    writeln!(file, "{line}").map_err(|error| format!("写 {} 失败：{error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 临时工作目录。名字带上进程号与序号，测试并行时不会互相踩。
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "refind-note-workspace-test-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn opening_creates_the_directory_skeleton() {
        let root = scratch("skeleton");
        let workspace = Workspace::open(root.clone()).unwrap();

        assert!(root.is_dir());
        assert!(workspace.database_dir().is_dir());
        assert!(workspace.settings_dir().is_dir());
        assert_eq!(workspace.root(), root.as_path());

        let _ = fs::remove_dir_all(&root);
    }

    /// 落盘的文件**只有本人读得动** —— 而且是**创建那一刻**就那样。
    ///
    /// 这一条盯的是"写完再 chmod"那种补法：它有个窗口，进程死在窗口里
    /// 那份文件就永远宽着（而 `settings/sync.json` 里是云端钥匙与 S3 私钥）。
    ///
    /// 只在 unix 上有意义（`PermissionsExt` 那套），别的平台跳过。
    #[cfg(unix)]
    #[test]
    fn what_gets_written_is_readable_by_its_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let root = scratch("private");
        let workspace = Workspace::open(root.clone()).unwrap();

        let target = workspace.settings_file("sync.json");
        write_json(&target, &serde_json::json!({ "key": "秘密" })).unwrap();

        let mode = fs::metadata(&target).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "落盘的文件宽了：{mode:o}");

        // 崩溃残留的临时文件也一样：那次 `mode` 对它不生效，靠的是显式那一次
        let leftover = target.with_extension("tmp");
        fs::write(&leftover, vec![b'x'; 8]).unwrap();
        fs::set_permissions(&leftover, fs::Permissions::from_mode(0o644)).unwrap();
        write_json(&target, &serde_json::json!({ "key": "还是秘密" })).unwrap();
        let mode = fs::metadata(&target).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "重写之后宽了：{mode:o}");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_missing_file_reads_as_default() {
        let root = scratch("missing");
        let workspace = Workspace::open(root.clone()).unwrap();

        let value: u32 = read_json(&workspace.settings_file("nothing.json"));
        assert_eq!(value, 0);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn written_json_survives_a_round_trip() {
        let root = scratch("roundtrip");
        let workspace = Workspace::open(root.clone()).unwrap();
        let path = workspace.settings_file("value.json");

        write_json(&path, &vec!["甲", "乙"]).unwrap();
        let read: Vec<String> = read_json(&path);
        assert_eq!(read, vec!["甲", "乙"]);

        // 临时文件不留下
        assert!(!workspace.settings_file("value.tmp").exists());

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn broken_json_reads_as_default_instead_of_failing() {
        let root = scratch("broken");
        let workspace = Workspace::open(root.clone()).unwrap();
        let path = workspace.settings_file("broken.json");

        fs::write(&path, "{ 这不是 JSON").unwrap();
        let value: u32 = read_json(&path);
        assert_eq!(value, 0);

        let _ = fs::remove_dir_all(&root);
    }
}
