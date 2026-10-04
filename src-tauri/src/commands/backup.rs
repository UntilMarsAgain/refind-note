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

//! 整仓库打包（迁移 / 备份）。

use tauri::AppHandle;

use crate::storage::workspace::Workspace;
use crate::storage::zip;

/// 默认文件名：带日期，免得一周内打包几次就分不清哪是哪份。
///
/// 日期取 **UTC**，不取本地时间：`time` 的本地时间要开 `local-offset` feature
/// （还得处理"这台机器的时区存不存在"），而为一个**文件名**不值得。
/// 顶多跨在午夜那几个小时的日期差一天 —— 对"这是哪一天的备份"没有实际影响。
fn default_name() -> String {
    let today = time::OffsetDateTime::now_utc()
        .format(&time::macros::format_description!("[year]-[month]-[day]"))
        .unwrap_or_else(|_| String::from("备份"));
    format!("refind-note-{today}.zip")
}

/// 把整个仓库打成一个 zip。
///
/// 没给 `target`（手机上没有系统保存对话框那一步）就落到下载目录 ——
/// 与导出笔记同一条路，见 [`crate::platform::saving::resolve`]。
///
/// 返回**真正写下去的那个路径**。
///
/// ## 会顺带排除什么
///
/// `staging/`（导出临时目录）、`.DS_Store`/`Thumbs.db`、`*.tmp`，
/// 以及**上一份备份**（`refind-note-*.zip`）—— 理由见 [`zip::write_zip`]。
///
/// ## 大仓库要有点耐心
///
/// 整个仓库可能几百 MB 到几 GB（附件与历史都在里面），一次压缩要几十秒。
/// 所以这个命令**只管写**：界面应当把进度说清楚，而不是让按钮看着像没反应。
#[tauri::command]
pub fn export_repository(app: AppHandle, target: Option<String>) -> Result<String, String> {
    let workspace = Workspace::open_default()?;

    // 路径先定：打包本身要写文件，得知道写哪儿
    let target = crate::platform::saving::resolve(&app, target, &default_name())?;

    let (files, bytes) = zip::write_zip(workspace.root(), &target)?;

    eprintln!(
        "[备份] 已打包 {files} 个文件、{} 字节 → {}",
        bytes,
        target.display()
    );
    Ok(target.to_string_lossy().to_string())
}
