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

//! 这台设备是什么 —— 界面偶尔要按它换一条路。

/// `"desktop"` 或 `"mobile"`（编译期就定了，不是运行时探测）。
///
/// 现在只有一处用它：另存为。桌面上弹系统保存对话框让人挑位置；手机上没那回事，
/// 统一由后端放进下载目录（见 [`crate::platform::saving`]）。
#[tauri::command]
pub fn platform_kind() -> &'static str {
    crate::platform::kind()
}
