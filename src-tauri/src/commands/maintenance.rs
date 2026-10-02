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

//! 回收站与整理。

use crate::features::maintenance;
use crate::open_database;

/// 回收站里的条目（新的在前）
#[tauri::command]
pub fn list_trash() -> Result<Vec<maintenance::TrashEntry>, String> {
    let (_, database) = open_database()?;
    database.list_trash()
}

/// 还原一条：日志挪回 `objects/`，并补一版"从回收站还原"
#[tauri::command]
pub fn restore_note(title: String) -> Result<(), String> {
    let (_, database) = open_database()?;
    database.restore(&title)
}

/// 永久清除一条（**不可撤销**；它引用的内容块留给整理那一轮去回收）
#[tauri::command]
pub fn purge_trash_entry(title: String) -> Result<(), String> {
    let (_, database) = open_database()?;
    database.purge_trash_entry(&title)
}

/// 清掉回收站里超过 `older_than_days` 天的条目（`0` = 全部）
#[tauri::command]
pub fn purge_trash(older_than_days: i64) -> Result<maintenance::PurgeReport, String> {
    let (_, database) = open_database()?;
    database.purge_trash(older_than_days)
}

/// 整理一遍：回收没人引用的内容块、清掉没有主的草稿槽位
#[tauri::command]
pub fn gc(orphan_blobs: bool, orphan_drafts: bool) -> Result<maintenance::GcReport, String> {
    let (_, database) = open_database()?;
    database.gc(orphan_blobs, orphan_drafts)
}

/// 开机自动维护：到期才做，没到期返回 null
#[tauri::command]
pub fn run_maintenance() -> Result<Option<maintenance::MaintenanceReport>, String> {
    let (_, database) = open_database()?;
    database.run_maintenance()
}
