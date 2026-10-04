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

//! 导出成哪种格式：markdown / html / pdf。
//!
//! ## 为什么不写在导出命令里
//!
//! 三种格式的**差别只有"写出什么"**，而"读那一版、确认没上锁、决定文件名"
//! 是共用的。把格式做成一个枚举，让调用处传一个值，比在导出函数里堆三个分支
//! 更容易看出"格式"这件事本身没有别的含义 —— 加第四种（`epub` 之类）时
//! 也只需在这里加一条。
//!
//! ## 词法顺序即取舍
//!
//! 单元测按"给出错误格式"来断言，而不是"这个枚举有三个变体" —— 前者在
//! 有人手滑加了第四种时**照样有意义**，后者只会失败。

use std::fmt;

/// 导出格式。
///
/// 序列化用 `kebab-case`（`markdown` / `html` / `pdf`）—— 前端传的就是这三个词，
/// 与界面上的选项一一对应，中间不必再换名字。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    /// markdown 原文：笔记里写下的那些字，拿到别处照样读得懂
    Markdown,
    /// 渲染好的 HTML：带样式与内部链接，浏览器直接能看
    Html,
    /// 打印用的 PDF
    Pdf,
}

impl ExportFormat {
    /// 认这三个词（大小写宽松：`PDF` 也认）。
    ///
    /// 传别的一律**报错**而不是退回 markdown：静默换成另一种格式，导出来的东西
    /// 与用户点的不是一回事，而那要到"打开一看才知道"。
    pub fn parse(text: &str) -> Result<Self, String> {
        match text.trim().to_ascii_lowercase().as_str() {
            "markdown" | "md" => Ok(Self::Markdown),
            "html" | "htm" => Ok(Self::Html),
            "pdf" => Ok(Self::Pdf),
            other => Err(format!(
                "不认识的导出格式「{other}」：只有 markdown / html / pdf"
            )),
        }
    }

    /// 文件扩展名（**含点**）。
    pub fn extension(self) -> &'static str {
        match self {
            Self::Markdown => ".md",
            Self::Html => ".html",
            Self::Pdf => ".pdf",
        }
    }

    /// 给一个**不带扩展名**的名字补上这一种的扩展名。
    ///
    /// 顺带处理"已经带着别的扩展名"的情况：用户可能已经在保存对话框里
    /// 填了 `笔记.md`，改成 html 时不该变成 `笔记.html.md`。
    ///
    /// ⚠️ 去扩展名**不能**用 `trim_end_matches(['.', 'm', 'd', ...])` ——
    /// 那是**字符集合**逐个剥，不是按后缀匹配，于是 `笔记.html` 会被剥成 `笔记.`
    /// （`l`/`m`/`o`… 不在集合里、`h`/`t`/`m`/`l` 在，结果乱剥一通）。
    /// 正确做法是**看最后一个点之后的那一段是不是已知扩展名**。
    pub fn extensionless_name(self, base: &str) -> String {
        let stem = strip_known_extension(base);
        format!("{stem}{}", self.extension())
    }
}

/// 去掉末尾**已知**的扩展名；不是扩展名就原样返回。
///
/// 判据是"最后一个点之后那一段是不是我们认得的那些扩展名" —— 而不是
/// "有没有点"。后者会把 `2026.10.04 日记` 的日期当扩展名剥掉。
fn strip_known_extension(name: &str) -> &str {
    const KNOWN: &[&str] = &[
        "md", "markdown", "html", "htm", "pdf", "txt", "text", "json", "zip",
    ];
    match name.rsplit_once('.') {
        Some((stem, suffix)) if KNOWN.contains(&suffix.to_ascii_lowercase().as_str()) => stem,
        _ => name,
    }
}

impl fmt::Display for ExportFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Markdown => "markdown",
            Self::Html => "html",
            Self::Pdf => "pdf",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::ExportFormat;

    #[test]
    fn the_three_words_round_trip() {
        for (text, expected, extension) in [
            ("markdown", ExportFormat::Markdown, ".md"),
            ("html", ExportFormat::Html, ".html"),
            ("pdf", ExportFormat::Pdf, ".pdf"),
        ] {
            assert_eq!(text, expected.to_string(), "显示出来的词要与认进去的一致");
            assert_eq!(expected, ExportFormat::parse(text).unwrap());
            assert_eq!(expected.extension(), extension);
        }
    }

    #[test]
    fn case_and_surrounding_space_do_not_matter() {
        // 前端传的值经过 JSON 与 trim，大小写不该成为一次失败的导出
        assert_eq!(ExportFormat::parse("  PDF ").unwrap(), ExportFormat::Pdf);
        assert_eq!(
            ExportFormat::parse("Markdown").unwrap(),
            ExportFormat::Markdown
        );
        // 常见的别名（对话框里人自己敲的）
        assert_eq!(ExportFormat::parse("md").unwrap(), ExportFormat::Markdown);
        assert_eq!(ExportFormat::parse("htm").unwrap(), ExportFormat::Html);
    }

    #[test]
    fn an_unknown_format_is_refused_rather_than_defaulted() {
        // 静默退回 markdown 最糟：导出来的东西与用户点的不是一回事
        let error = ExportFormat::parse("epub").unwrap_err();
        assert!(error.contains("epub"), "报错要说清是什么不认识：{error}");
        assert!(error.contains("markdown"), "报错要说清有哪些可选：{error}");
        assert!(ExportFormat::parse("").is_err());
    }

    #[test]
    fn the_extension_is_replaced_not_appended() {
        // 用户可能已经在对话框里填了 `.md`
        assert_eq!(
            ExportFormat::Html.extensionless_name("笔记.md"),
            "笔记.html"
        );
        assert_eq!(
            ExportFormat::Pdf.extensionless_name("笔记.html"),
            "笔记.pdf"
        );
        assert_eq!(ExportFormat::Markdown.extensionless_name("笔记"), "笔记.md");
        // 标题本身没有扩展名时也别多一个点
        assert_eq!(
            ExportFormat::Html.extensionless_name("2026.10.04 日记"),
            "2026.10.04 日记.html"
        );
    }

    #[test]
    fn a_date_in_the_title_is_not_mistaken_for_an_extension() {
        // 标题里带日期很常见（"2026.10.04 日记"）。
        // 判据要是"有没有点"而不是"最后一段认不认得"，这一天就会被剥掉。
        assert_eq!(
            ExportFormat::Pdf.extensionless_name("2026.10.04 日记"),
            "2026.10.04 日记.pdf"
        );
        assert_eq!(
            ExportFormat::Html.extensionless_name("v1.2.3 复盘"),
            "v1.2.3 复盘.html"
        );
    }

    #[test]
    fn unrelated_trailing_words_stay_put() {
        // 不认识的扩展名不算扩展名 —— 用户可能就想要 `笔记.final.html`
        assert_eq!(
            ExportFormat::Pdf.extensionless_name("笔记.final"),
            "笔记.final.pdf"
        );
        // 大写也算扩展名（从别的系统拷过来的文件名常是大写后缀）
        assert_eq!(
            ExportFormat::Html.extensionless_name("笔记.MD"),
            "笔记.html"
        );
    }
}
