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

//! 工作目录、偏好与默认策略（启动那几件事与设置页）。

use crate::open_database;
use serde::Serialize;

use crate::settings::{self, Preferences};
use crate::storage::codec::Policy;
use crate::vault::database::Meta;

#[derive(Serialize)]
pub struct WorkspaceInfo {
    /// 工作目录（给人看的位置）
    root: String,
    /// 数据库目录
    database_root: String,
    meta: Meta,
    preferences: Preferences,
    /// 仓库默认的保护策略（新笔记从它出发）
    protection: Policy,
    /// 这台计算机上有没有 gpg（没有时签名 / 加密不可用）
    gpg_available: bool,
    /// 仓库的整理设置：回收站留多少天、自动整理隔多少天、上次各是什么时候
    maintenance: MaintenanceInfo,
}

#[derive(Serialize)]
pub struct MaintenanceInfo {
    trash_keep_days: u64,
    gc_interval_days: u64,
    last_trash_purge: String,
    last_gc: String,
}

#[tauri::command]
pub fn open_workspace() -> Result<WorkspaceInfo, String> {
    let (workspace, database) = open_database()?;
    Ok(WorkspaceInfo {
        root: workspace.root().display().to_string(),
        database_root: database.root().display().to_string(),
        meta: database.meta().clone(),
        preferences: settings::load(&workspace)?,
        protection: database.protection(),
        gpg_available: database.gpg_available(),
        maintenance: {
            let config = database.config();
            MaintenanceInfo {
                trash_keep_days: config.trash_keep_days,
                gc_interval_days: config.gc_interval_days,
                last_trash_purge: config.last_trash_purge,
                last_gc: config.last_gc,
            }
        },
    })
}

#[tauri::command]
pub fn save_preferences(preferences: Preferences) -> Result<Preferences, String> {
    let (workspace, _) = open_database()?;
    settings::save(&workspace, preferences)
}

#[tauri::command]
pub fn set_protection(protection: Policy) -> Result<(), String> {
    let (_, database) = open_database()?;
    let mut config = database.config();
    config.protection = protection;
    database.save_config(&config)
}

/// 改整理设置：回收站留多少天、自动整理隔多少天（都不小于 1 天）
#[tauri::command]
pub fn set_maintenance(trash_keep_days: u64, gc_interval_days: u64) -> Result<(), String> {
    let (_, database) = open_database()?;
    let mut config = database.config();
    config.trash_keep_days = trash_keep_days.max(1);
    config.gc_interval_days = gc_interval_days.max(1);
    database.save_config(&config)
}
