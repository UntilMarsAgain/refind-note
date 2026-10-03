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

//! 模板标准库：随程序自带的那几个模板。
//!
//! 与指令表同样的规矩：**有什么模板只写在这一个地方** —— 下面那张 [`TEMPLATES`] 表。
//! 加一个模板 = 写一个渲染器 + 在表里加一行（顺序也照旧，界面与错误提示会按它列出来）。
//!
//! 渲染器按**块内容被怎么处理**分成四组，一个模板的写法几乎全由这一点决定 —— 所以分组的
//! 依据是职责，不是数量：
//!
//! - [`text`]：只写文字的块。内容**照常按 markdown 解析**（`quote` / `aside` / `title` …），
//!   参数只管外框的样子：`align`、`color=`、`height=` 之类。
//! - [`media`]：把一个文件摆出来（`image` / `video` / `audio`）。三者共用一套 `src=` 校验、
//!   尺寸上限与图注规矩，所以放在同一个文件里 —— 分开写就会各抄一份，那套规矩迟早走样。
//! - [`panels`]：**按 `[标签]` 分节**的块（`tabs` / `theme`）。全库只有这两个，见 [`SECTIONED`]；
//!   它们不读块正文，而是各节已解析好的子节点。
//! - [`embeds`]：把内容**原样写出去**的块（`code` / `mermaid` / `math` / `css` / `html` / `js`）。
//!   正文一概不按 markdown 解析：代码、图定义、TeX、样式里的记号都是它们自己的。
//! - [`callouts`]：提示框（`note` / `tip` / `warning` / `danger` / `error`）。**形状完全
//!   一样**，差别只是"哪一种"，所以共用一个渲染器 + 一张五行的小表；加第六种 = 加一行。
//!
//! 留在本文件的三个公共件：[`size_rule`]`（尺寸参数 → 一条 CSS 声明）、
//! [`normalize_hex`]（`color=` 只认十六进制）与 [`text_on`]（底色 → 字色）。它们都是
//! **多组共用**的：尺寸是文字组与媒体组都要过，颜色的两个是 `::banner` 与提示框都要过。
//! 放在任一组都会让另一组跨界来取，所以它们是这一层的公共件。
mod callouts;
mod embeds;
mod media;
mod panels;
mod text;

use super::dispatch::{render_problem_page, TemplateRenderer};
use callouts::{
    render_danger, render_error, render_note, render_tip, render_warning,
};
use embeds::{render_code, render_css, render_html, render_js, render_math, render_mermaid};
use media::{render_audio, render_image, render_video};
use panels::{render_tabs, render_theme};
use text::{
    render_aside, render_banner, render_fields, render_quote, render_signature, render_title,
};

/// 全部标准模板。
pub static TEMPLATES: &[(&str, TemplateRenderer)] = &[
    ("quote", render_quote),
    ("code", render_code),
    ("aside", render_aside),
    ("fields", render_fields),
    ("banner", render_banner),
    ("title", render_title),
    ("tabs", render_tabs),
    ("theme", render_theme),
    ("image", render_image),
    ("video", render_video),
    ("audio", render_audio),
    ("css", render_css),
    ("html", render_html),
    ("js", render_js),
    ("mermaid", render_mermaid),
    ("math", render_math),
    ("signature", render_signature),
    ("note", render_note),
    ("tip", render_tip),
    ("warning", render_warning),
    ("danger", render_danger),
    ("error", render_error),
    // 不是给人写笔记用的模板，但**必须真的在表里**：`expand::text_node` 造的就是
    // 一个 `name: "problem"` 的节点，分发只认这张表 —— 它不在，"模板页没解锁"就会被
    // 显示成"未知模板 :: problem"，把作者引到错的地方去。见 dispatch::render_problem_page。
    ("problem", render_problem_page),
];

/// 内容按 `[标签]` 分节的模板。
///
/// 分节发生在**扫描时**（各节要各自解析成 markdown，见 `mod.rs`），所以扫描器得知道
/// 哪些模板分节 —— 这张表因此和 `TEMPLATES` 放在一起：两张表加起来才是"有哪些模板"。
/// 分节模板的渲染器在 [`panels`] 里。
pub static SECTIONED: &[&str] = &["tabs", "theme"];

/// 这个名字是内置模板吗（不是的话，扫描器会去 `Template:` 命名空间找同名的页）
pub fn is_builtin(name: &str) -> bool {
    TEMPLATES.iter().any(|(builtin, _)| *builtin == name)
}

/// 这个模板是不是分节的（扫描器用）
pub fn takes_sections(name: &str) -> bool {
    SECTIONED.contains(&name)
}

/// 尺寸参数 → 一条 CSS 声明。只认"数字 + 可选白名单单位"，别的写法一律不认。
///
/// 严格是有理由的：这个值会被写进 `style` 属性，宽松了就等于允许往里面塞任意声明
/// （比如 `width="1px; background: url(//追踪地址)"`）。
pub(super) fn size_rule(name: &str, value: &str) -> Option<String> {
    let trimmed = value.trim();
    let digits: String = trimmed
        .chars()
        .take_while(|ch| ch.is_ascii_digit() || *ch == '.')
        .collect();
    if digits.is_empty() || digits.parse::<f64>().is_err() {
        return None;
    }
    let unit = &trimmed[digits.len()..];
    if !matches!(unit, "" | "px" | "%" | "em" | "rem" | "vh" | "vw") {
        return None;
    }
    let unit = if unit.is_empty() { "px" } else { unit };
    Some(format!("{name}: {digits}{unit};"))
}

/// `#abc` / `#aabbcc` → 规范的 `#aabbcc`；别的写法一律不认。
///
/// 放在这一层而不是 [`text`] 里：`color=` 有两组模板认它（`::banner` 与提示框那一族），
/// 留在任一组都会让另一组跨界来取 —— 与 [`size_rule`] 是同一个理由。
pub(super) fn normalize_hex(text: &str) -> Option<String> {
    let body = text.trim().strip_prefix('#')?;
    let expanded: String = match body.len() {
        3 => body.chars().flat_map(|ch| [ch, ch]).collect(),
        6 => body.to_string(),
        _ => return None,
    };
    if !expanded.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return None;
    }
    Some(format!("#{}", expanded.to_lowercase()))
}

/// 底色 → 该配什么颜色的字。
///
/// 作者只给底色，字色由这里定：深底配深字是最常见的"自己给自己挖坑"。
/// 用 sRGB 亮度的常见近似，够用且一眼能看懂为什么这么算。
pub(super) fn text_on(background: &str) -> &'static str {
    let Some(body) = background.strip_prefix('#') else {
        return "var(--text)";
    };
    let Ok(value) = u32::from_str_radix(body, 16) else {
        return "var(--text)";
    };
    let (r, g, b) = ((value >> 16) & 0xff, (value >> 8) & 0xff, value & 0xff);
    let luminance = (0.299 * r as f64 + 0.587 * g as f64 + 0.114 * b as f64) / 255.0;
    if luminance > 0.6 {
        "#101010"
    } else {
        "#f5f5f5"
    }
}
