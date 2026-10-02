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

//! GPG 密钥：列出、导入、删除。

use crate::features::keys;

/// 钥匙串里的钥匙（新的在前）；没有 gpg 时是空列表
#[tauri::command]
pub fn gpg_keys() -> Result<Vec<keys::GpgKey>, String> {
    keys::list()
}

/// 从一份钥匙文件导入
#[tauri::command]
pub fn import_gpg_key(path: String) -> Result<keys::ImportSummary, String> {
    let bytes = std::fs::read(&path).map_err(|error| format!("读不到这个文件：{error}"))?;
    keys::import(&bytes)
}

/// 删掉一把公钥（带私钥的不动）
#[tauri::command]
pub fn delete_gpg_key(fingerprint: String) -> Result<(), String> {
    keys::delete(&fingerprint)
}
