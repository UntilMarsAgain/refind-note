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

//! 交给系统默认应用打开时，先把内容落到一个临时文件上。
//!
//! 仓库里存的是字节，没有一个"能在文件管理器里双击"的路径 —— 所以这一步是必须的。
//! 落在临时目录里是**刻意**的：它是给外部程序看的副本，用完由系统回收，
//! 不进仓库、也不该被当成原件的家。加密存的内容到了这一步已经是明文，
//! 所以权限收紧到只有本人可读。

/// 落临时文件的地方：桌面上是系统的临时目录，**移动端是程序自己的缓存目录**
/// （手机上 `/tmp` 不存在、也写不进去）。移动端在启动时由 `lib.rs` 告诉我们。
static SCRATCH: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

/// 定下临时目录（只认第一次；移动端在启动时叫一次）
pub fn install_scratch(dir: std::path::PathBuf) {
    let _ = SCRATCH.set(dir);
}

/// 这一台设备上放临时东西的地方（诊断页要报它：手机上不是 `/tmp`）
pub fn scratch_dir() -> std::path::PathBuf {
    SCRATCH.get().cloned().unwrap_or_else(std::env::temp_dir)
}

/// 把一个文件的字节落到临时目录里，返回那个路径。
///
/// 名字取页面名（后缀要留着：系统靠它挑应用）。**权限收紧到只有本人可读** ——
/// 从仓库里出来的可能是解过密的明文，临时目录别的人也可能看得到。
pub fn stage_file(name: &str, bytes: &[u8]) -> Result<std::path::PathBuf, String> {
    let directory = scratch_dir().join("refind-note-open");
    std::fs::create_dir_all(&directory)
        .map_err(|error| format!("建不出临时目录 {}：{error}", directory.display()))?;

    // **只取最后一段**：这一层是拿一个名字去拼路径落盘，所以"这个名字能走到哪"
    // 得在这一层说了算，不能指望每个调用方都先洗一遍。
    // （页面名那条路已经洗过：`features/files.rs` 的 `clean_name` 会去掉路径部分
    // 与首尾的点 —— 但那是**另一层**的约定，隔一层就指望不上了。）
    let tail = std::path::Path::new(name)
        .file_name()
        .and_then(|tail| tail.to_str())
        .map(str::trim)
        .unwrap_or("");
    if tail.is_empty() || tail.chars().all(|ch| ch == '.') {
        return Err(format!("拿这个名字落不了临时文件：{name:?}"));
    }

    let path = directory.join(tail);
    crate::storage::workspace::write_bytes(&path, bytes)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .map_err(|error| format!("改权限失败：{error}"))?;
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 装一次的那个暂存目录 —— **两个用例共用同一个**。
    ///
    /// [`SCRATCH`] 是 `OnceLock`、[`install_scratch`] 只认第一次（生产代码里
    /// 由 `lib.rs` 在启动时调一次）。所以用例里若各装各的目录，**第二个**
    /// 装的不生效，而它的断言还按自己那个目录算 —— 于是"谁先跑"决定成败，
    /// 也就是一条**时好时坏**的测试（自己踩过一次：整轮跑里偶发红、单跑绿）。
    ///
    /// 共用一个目录就没有顺序可说了：装一次，之后都拿它。
    fn staging_root() -> std::path::PathBuf {
        static ROOT: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
        ROOT.get_or_init(|| {
            let dir = std::env::temp_dir()
                .join(format!("refind-note-staging-test-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            install_scratch(dir.clone());
            dir
        })
        .clone()
    }

    /// 落下来的那份是**明文**（从仓库里出来的可能是解过密的内容），
    /// 而它躺在**系统的临时目录**里 —— 那地方别人也可能看得到。
    /// 所以这一层唯一值得盯的性质就是：权限收紧到只有本人可读。
    ///
    /// 顺带钉住两件小事：后缀留着（系统靠它挑应用），以及名字里带路径的
    /// 写法不该写到临时目录外面去（`join` 一个 `../..` 是能跑出去的）。
    #[cfg(unix)]
    #[test]
    fn a_staged_file_is_readable_by_its_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let path = stage_file("桥.png", b"\x89PNG\r\n").unwrap();
        assert!(path.is_file(), "没落下来：{}", path.display());

        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "临时副本宽了：{mode:o}");

        // 内容原样，后缀也留着（系统靠它挑应用）
        assert_eq!(std::fs::read(&path).unwrap(), b"\x89PNG\r\n");
        assert_eq!(path.extension().and_then(|it| it.to_str()), Some("png"));
    }

    /// 名字里带路径的，落在**最后一段**上。
    ///
    /// 这一层是拿一个名字去拼路径落盘的，所以"这个名字能走到哪"得在**这里**
    /// 有个说法 —— 页面名那条路确实洗过（`features/files.rs` 的 `clean_name`
    /// 去掉路径部分与首尾的点），但那是另一层的约定，隔一层就指望不上了。
    #[cfg(unix)]
    #[test]
    fn a_name_cannot_escape_the_staging_directory() {
        let staging = staging_root().join("refind-note-open");

        for name in ["../逃出去了.txt", "a/b/c.txt", "..\\逃出去了.txt"] {
            let path = stage_file(name, b"x").unwrap();
            assert_eq!(
                path.parent(),
                Some(staging.as_path()),
                "{name:?} 落到了临时目录外面：{}",
                path.display()
            );
        }

        // 什么都不是的名字直接报错，而不是落一个叫 `.` 的文件
        assert!(stage_file("..", b"x").is_err());
        assert!(stage_file("", b"x").is_err());
        assert!(stage_file("/", b"x").is_err());
    }
}
