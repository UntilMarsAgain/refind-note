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

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::Serialize;

/// 工作目录名（放在用户主目录下）
pub const DIR_NAME: &str = ".refind-note";

/// 数据库文件夹
const DB_DIR: &str = "db";

/// 设置与浏览状态文件夹
const SETTINGS_DIR: &str = "settings";

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

    /// 打开默认工作目录：`~/.refind-note`
    pub fn open_default() -> Result<Self, String> {
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

/// 用户主目录
fn home_dir() -> Result<PathBuf, String> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .ok_or_else(|| "找不到用户主目录（HOME / USERPROFILE 都没有）".to_string())
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

    fs::write(&temporary, bytes)
        .map_err(|error| format!("写入 {} 失败：{error}", temporary.display()))?;
    fs::rename(&temporary, path)
        .map_err(|error| format!("落盘 {} 失败：{error}", path.display()))?;

    Ok(())
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
