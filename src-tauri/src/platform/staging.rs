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
    let path = directory.join(name);
    crate::storage::workspace::write_bytes(&path, bytes)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .map_err(|error| format!("改权限失败：{error}"))?;
    }
    Ok(path)
}
