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

//! 帮助页：随程序发布的那几页（只读）。

use crate::features;
use crate::open_database;

/// 帮助页清单（**菜单里那几页**；页面名与标题，正文也一并给出，页数不多）
#[tauri::command]
pub fn help_pages() -> Result<Vec<features::help::HelpPage>, String> {
    let (_, database) = open_database()?;
    Ok(features::help::pages(&database))
}

/// `help/` 目录下**全部**的页（按页面名排）
///
/// 与 [`help_pages`] 的区别是"菜单里列没列它"，不是"打不打得开"——
/// 两者都能打开，只是这一个列全了。给「全部页面」那一页用：
/// 它回答的是"这儿有哪些页面"，清单不完整就是骗人。
#[tauri::command]
pub fn all_help_pages() -> Result<Vec<features::help::HelpPage>, String> {
    let (_, database) = open_database()?;
    Ok(features::help::every_page(&database))
}

/// 读一页帮助（含渲染好的 HTML）
#[tauri::command]
pub fn read_help(page: String) -> Result<features::help::HelpPage, String> {
    let (_, database) = open_database()?;
    features::help::find(&database, &page).ok_or_else(|| format!("没有这页帮助：{page}"))
}
