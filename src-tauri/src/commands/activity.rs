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

//! 最近更改、浏览历史、特殊页面清单。

use crate::features::{browsing, changes};
use crate::open_database;
use crate::vault::address;

/// 最近的改动（跨全部笔记，新的在前）
#[tauri::command]
pub fn recent_changes(
    limit: usize,
    include_drafts: bool,
) -> Result<Vec<changes::ChangeEntry>, String> {
    let (_, database) = open_database()?;
    database.recent_changes(limit, include_drafts)
}

/// 浏览历史（新的在前）
#[tauri::command]
pub fn browsing_history() -> Result<Vec<browsing::Visit>, String> {
    let (_, database) = open_database()?;
    Ok(database.browsing())
}

/// 记一次访问；返回记完之后的整份清单
#[tauri::command]
pub fn record_visit(address: String, title: String) -> Result<Vec<browsing::Visit>, String> {
    let (_, database) = open_database()?;
    database.record_visit(&address, &title)
}

/// 清空浏览历史
#[tauri::command]
pub fn clear_history() -> Result<(), String> {
    let (_, database) = open_database()?;
    database.clear_browsing()
}

/// 现有的特殊页面（菜单据此生成）
#[tauri::command]
pub fn special_pages() -> Vec<String> {
    address::SPECIAL_PAGES
        .iter()
        .map(|page| page.to_string())
        .collect()
}
