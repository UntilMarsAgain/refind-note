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

//! 自定义语法。
//!
//! 约定：每个语法一个文件，对外暴露 `add(md)`；统一在 [`register`] 里挂载。
//! 新增一个语法 = 加一个文件 + 在 `register` 里加一行。

pub mod math;
pub mod template;
pub mod wikilink;

use markdown_it::MarkdownIt;

/// 所有自定义语法注册到解析器上。
pub fn register(md: &mut MarkdownIt) {
    math::add(md);
    template::add(md);
    wikilink::add(md);
}

/// 这一层带了哪几种语法（诊断页要报）。
///
/// 与上面 `register` 里那三行一一对应 —— 加一种就两边一起加。
pub fn names() -> [&'static str; 3] {
    [
        "math（$…$ / $$…$$）",
        "template（::名字）",
        "wikilink（[[目标]]）",
    ]
}
