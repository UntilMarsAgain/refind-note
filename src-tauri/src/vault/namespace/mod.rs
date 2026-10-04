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

//! 命名空间：标题前缀那张表。
//!
//! 一篇笔记的身份是**两段**：命名空间 + 页面名。地址里的前缀（`帮助:入门`）说的就是
//! 前一段。这张表把「人写的前缀」与「内部标识」分开记：
//!
//! - **标识**（`id`）是稳定的：目录名（`objects/<标识>/`）、键（`<标识>:<页面名>`）都用它，
//!   所以**改名不动任何文件**；
//! - **名称 / 别名**是给人写的：地址栏、链接、列表里显示的都是它，大小写与首尾空白
//!   不影响匹配。
//!
//! 四个内建的：主命名空间（没有前缀）、`special`（虚拟，页面由程序提供）、
//! `help`（虚拟，页面是编进程序的用户帮助）、
//! `template`（保留，模板与样式放这里）。其余的可以自己建：内容命名空间落在本仓库，
//! 也可以给一个站点地址模板，那就成了**跨站命名空间**（只用于链接，页面不在本仓库）。
//!
//! # 分层
//!
//! 这个文件只留**模型本身**——数据形状、内建的那几个、以及常量。其余分两处：
//!
//! - [`table`]：**纯内存**的那张表怎么读、怎么认一个名字、怎么改。凡是不碰磁盘的都
//!   在这里，所以它可以脱离仓库单测。
//! - [`repository`]：这张表在**仓库上**怎么落 —— 建目录、改名时顺带把标题里的前缀
//!   一起改掉。
//!
//! 边界：表不认识仓库，仓库也不解释表内的约束（重名、保留名那些规则都在
//! [`table`] 里）。跨站链接的**页面名转义**（`encode_page`）跟着 [`table`] 走，
//! 因为它属于「怎么把名字写进地址」的规则，不属于落盘。

#[cfg(test)]
mod tests;

mod repository;
mod table;

use serde::{Deserialize, Serialize};

use table::encode_page;

/// 主命名空间的标识（也是它目录的名字）。它没有前缀。
pub const MAIN_ID: &str = "0";
/// 虚拟命名空间 `special` 的标识：里面的页面由程序提供，不落仓库。
pub const SPECIAL_ID: &str = "special";
/// 保留命名空间 `template` 的标识。
pub const TEMPLATE_ID: &str = "template";

/// 模板命名空间在地址里写的那一段（`Template:卡片`）
pub const TEMPLATE_NAME: &str = "template";
/// 虚拟命名空间 `help` 的标识：里面的页面是**编进程序里的用户帮助**。
pub const HELP_ID: &str = "help";
/// 它的规范名（地址里写 `Help:入门`）
pub const HELP_NAME: &str = "Help";
/// 命名空间 `File` 的标识：里面的页面**正文是字节**（附件、图片）。
pub const FILE_ID: &str = "file";
/// 它的规范名（地址里写 `File:桥.png`）
pub const FILE_NAME: &str = "File";

/// 一个命名空间。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Namespace {
    /// 标识：稳定键，磁盘目录与标题键都用它。改名不动它
    pub id: String,
    /// 规范名（地址里的前缀）；主命名空间是空串
    pub name: String,
    /// 别名：也认，回显时**用写下来的那一个**（不换算成规范名）
    #[serde(default)]
    pub aliases: Vec<String>,
    /// 页面是否落在本仓库。虚拟与跨站是 `false`
    pub storable: bool,
    /// 跨站地址模板，`$1` 是页面名。只有跨站命名空间才有
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub site: Option<String>,
}

impl Namespace {
    /// 跨站命名空间：页面不在本仓库，只有链接出得去
    pub fn is_cross_site(&self) -> bool {
        self.site.is_some()
    }

    /// 按模板拼出跨站地址；没有模板就没有地址
    pub fn url_for(&self, page: &str) -> Option<String> {
        self.site
            .as_ref()
            .map(|template| template.replace("$1", &encode_page(page)))
    }
}

/// 整张表。落盘在 `db/namespaces.json`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NamespaceTable {
    pub items: Vec<Namespace>,
    /// 默认的跨站命名空间播种过没有。
    ///
    /// 播完置位，于是**删掉之后不会再长回来** —— 播种只发生在新仓库上。
    #[serde(default)]
    pub defaults_sown: bool,
}

impl Default for NamespaceTable {
    fn default() -> Self {
        Self::builtin()
    }
}
