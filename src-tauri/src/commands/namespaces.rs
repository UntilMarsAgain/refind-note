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

//! 命名空间表（标题前缀那张表）。

use crate::open_database;
use crate::vault::namespace::Namespace;

/// 命名空间表（标题前缀那张表）
#[tauri::command]
pub fn namespaces() -> Result<Vec<Namespace>, String> {
    let (_, database) = open_database()?;
    Ok(database.namespace_list())
}

/// 新建命名空间：`site` 给了就是跨站（页面不在本仓库）。返回更新后的整张表。
#[tauri::command]
pub fn add_namespace(
    name: String,
    aliases: Vec<String>,
    site: Option<String>,
) -> Result<Vec<Namespace>, String> {
    let (_, database) = open_database()?;
    database.add_namespace(&name, aliases, site)
}

/// 改别名与站点地址（名称与标识不动）
#[tauri::command]
pub fn update_namespace(
    key: String,
    aliases: Vec<String>,
    site: Option<String>,
) -> Result<Vec<Namespace>, String> {
    let (_, database) = open_database()?;
    database.update_namespace(&key, aliases, site)
}

/// 改名：不动文件，只改表里那一行与标题里的前缀
#[tauri::command]
pub fn rename_namespace(key: String, name: String) -> Result<Vec<Namespace>, String> {
    let (_, database) = open_database()?;
    database.rename_namespace(&key, &name)
}

/// 清空：里面的页面全部移进回收站
#[tauri::command]
pub fn empty_namespace(key: String) -> Result<usize, String> {
    let (_, database) = open_database()?;
    database.empty_namespace(&key)
}

/// 删除：先清空，再从表里去掉
#[tauri::command]
pub fn delete_namespace(key: String) -> Result<usize, String> {
    let (_, database) = open_database()?;
    database.delete_namespace(&key)
}
