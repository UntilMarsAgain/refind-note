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

//! 模板块：`::名字 参数=值 …` 开一个块，块的边界由**缩进**划出。
//!
//! ```text
//! ::name key=value key=value
//!   content
//! ```
//!
//! 块内容的边界规则只有一条：**缩进**。头行之后缩进更深的行属于块内，遇到不更深的行
//! 就结束，不需要结束标记。
//!
//! 一个模板块要走的全程分给几个子模块，本文件只留**类型与注册**，剩下的按下述分工：
//!
//! - [`parse`]：头解析（引号、转义）—— 这里能写的东西最多，所以单独一个文件；
//! - [`scanner`]：块级规则本身 —— 认出 `::` 那一行、往下量缩进、切分节；
//! - [`expand`]：**用户自定义模板**的展开（`Template:名字` 那一页的正文填好参数再解析）；
//! - [`fill`]：`{{}}` 填空与 HTML / CSS 过滤；
//! - [`dispatch`]：按名字分发，查不到就渲染"未知模板"的框；
//! - [`stdlib`]：模板标准库（`quote` 等），加模板只改那一个目录。
//!
//! 本文件留下的是共用件：[`Section`]（分节模板的一节）、[`Template`] 的渲染接线
//! （渲染即"按名字分发"）、[`names`]（诊断页要报的那一串）与 [`add`]（挂到解析器上）。
//! 块的识别细节全在 [`scanner`]，标准的名字在 [`stdlib`] —— 两边都不在这里。
mod dispatch;
mod expand;
mod fill;
mod parse;
mod scanner;
mod stdlib;

use markdown_it::MarkdownIt;
use markdown_it::{Node, NodeValue, Renderer};

pub use parse::Template;

/// 块级规则本体（挂在解析器上，见 [`add`]）。文档隐藏：它是插件的零件，不是接口。
#[doc(hidden)]
pub use scanner::TemplateScanner;

// 标准模板表对外公开：模板分发与调试都要按名字看这张表。
#[allow(unused_imports)]
pub use stdlib::TEMPLATES;

/// 分节模板（`::tabs` / `::theme`）里的一节：`[标签]` 开头的那一段。
///
/// 内容**各自**解析成子节点挂在它下面 —— 分节的意义就在这里：一节里的正文是一篇
/// 独立的小文档（可以写列表、链接，也可以再嵌模板），而不是大块里的一段普通文字。
#[derive(Debug, Clone, PartialEq)]
pub struct Section {
    /// `[标签]` 里的那几个字；写在第一个标签**之前**的内容，标签是空串
    pub label: String,
}

impl NodeValue for Section {
    fn render(&self, node: &Node, fmt: &mut dyn Renderer) {
        // 兜底：万一某个渲染器忘了拆节、直接把内容写出来，也不该整段消失
        fmt.contents(&node.children);
    }
}

/// 内置模板的名字（诊断页要报：模板出问题时，先看这一串对不对）
pub fn names() -> Vec<&'static str> {
    stdlib::TEMPLATES.iter().map(|(name, _)| *name).collect()
}

impl NodeValue for Template {
    fn render(&self, node: &Node, fmt: &mut dyn Renderer) {
        dispatch::render(self, node, fmt);
    }
}

/// 把模板块的识别规则挂到解析器上。
///
/// 挂载位置是刻意的：必须排在段落规则**之前**，否则 `::名字` 会先被当成一行普通文字收走
/// （见 [`scanner`]）。
pub fn add(md: &mut MarkdownIt) {
    // 必须排在段落规则之前：否则 `::名字` 会先被当成一行普通文字收走
    md.block
        .add_rule::<TemplateScanner>()
        .before::<markdown_it::plugins::cmark::block::paragraph::ParagraphScanner>()
        .after_all();
}
