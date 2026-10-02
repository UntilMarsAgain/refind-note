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

//! "另存为 / 导出"落到哪个文件上。
//!
//! 桌面上那一步是界面问出来的：系统保存对话框给出一个路径，原样交到这里。
//! **手机上没有那一步** —— Android 的文件选择器给回来的是 `content://…` 那种地址，
//! 不是能直接写的路径（而且那边也没有"选个文件夹"这回事）。所以手机上统一落到
//! **程序自己的下载目录**：`/storage/emulated/0/Android/data/<包名>/files/Download`，
//! 文件管理器看得见，又不需要任何权限。
//!
//! 两条路的终点都一样：返回**真正写下去的那个路径**，界面拿它说一句"存到哪儿了"。

use std::path::{Path, PathBuf};

use tauri::Manager as _;

/// 选定最终路径：给了就用给的，没给（手机上）就进下载目录。
///
/// 返回的是**最终路径**而不是"要不要另存为"——调用方把内容写到它上面就是了。
pub fn resolve(app: &tauri::AppHandle, chosen: Option<String>, suggested: &str) -> Result<PathBuf, String> {
    // `content://…` 不是路径（Android 的文件选择器给的就是这个）：当没给，
    // 照走下载目录 —— 宁可存到我们能写的地方，也不要在这儿失败
    if let Some(path) = chosen.filter(|path| !path.trim().is_empty() && !path.contains("://")) {
        return Ok(PathBuf::from(path));
    }
    into_downloads(app, suggested)
}

/// 下载目录里的一个位置（重名往后编号）
fn into_downloads(app: &tauri::AppHandle, name: &str) -> Result<PathBuf, String> {
    let directory = app
        .path()
        .download_dir()
        .map_err(|error| format!("这台设备上找不到下载目录：{error}"))?;
    std::fs::create_dir_all(&directory)
        .map_err(|error| format!("建不出下载目录 {}：{error}", directory.display()))?;
    Ok(unique(&directory, name))
}

/// 重名就往后编号：`桥.png` → `桥-2.png` → `桥-3.png`。
///
/// 覆盖别人的东西是不礼貌的，而"同一个名字存两次"在导出这件事上很常见
/// （今天导一遍、明天改完再导一遍）—— 编号让人看得见自己存了几份。
fn unique(directory: &Path, name: &str) -> PathBuf {
    let candidate = directory.join(name);
    if !candidate.exists() {
        return candidate;
    }

    // 拆成"主干 + 后缀"再编号：`桥.png` 是 `桥` + `.png`，不是 `桥.png` + 空
    let path = Path::new(name);
    let stem = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .unwrap_or_else(|| "导出".to_string());
    let extension = path.extension().map(|ext| ext.to_string_lossy().to_string());

    for index in 2..1000 {
        let numbered = match &extension {
            Some(extension) => format!("{stem}-{index}.{extension}"),
            None => format!("{stem}-{index}"),
        };
        let candidate = directory.join(numbered);
        if !candidate.exists() {
            return candidate;
        }
    }

    // 一千个重名：随它去，覆盖就覆盖（比转圈强）
    directory.join(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 重名往后编号；没重名就原样
    #[test]
    fn a_repeated_name_gets_a_number() {
        let dir = std::env::temp_dir().join(format!("refind-note-save-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        assert_eq!(unique(&dir, "桥.png"), dir.join("桥.png"));

        std::fs::write(dir.join("桥.png"), "第一份".as_bytes()).unwrap();
        assert_eq!(unique(&dir, "桥.png"), dir.join("桥-2.png"));

        std::fs::write(dir.join("桥-2.png"), "第二份".as_bytes()).unwrap();
        assert_eq!(unique(&dir, "桥.png"), dir.join("桥-3.png"));

        // 没有后缀的也一样
        std::fs::write(dir.join("落款"), "x".as_bytes()).unwrap();
        assert_eq!(unique(&dir, "落款"), dir.join("落款-2"));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
