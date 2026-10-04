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

//! 导出 PDF —— 但**不是**在 Rust 里生成 PDF 字节。
//!
//! ## 为什么绕这一圈
//!
//! 看上去最直接的路是调 WebView 的打印接口（`Webview::print()` / wry 的
//! `print_with_options`）。查过之后走不通，理由两条：
//!
//! 1. **它只能弹系统打印对话框，不能"印到某个文件"**。`PrintOptions` 里只有
//!    `margins` 一个字段，没有输出路径。而"导出 PDF"要的是后者。
//! 2. **它是同步返回的，对话框是异步的** —— 调用返回时用户还没选打印机，
//!    程序既不知道最终存到哪儿，也不知道印没印成。而导出命令的契约是
//!    **返回真正写下去的那个路径**（桌面端靠它填对话框、手机端靠它说"存到哪儿了"）。
//!
//! 在这个前提下"打印成 PDF"与"导出 PDF 到指定路径"是**两件不同的事**，
//! 硬凑只会得到一个成功返回却什么都没写的命令。
//!
//! ## 所以这里做什么
//!
//! **Rust 只负责给出一份"为打印准备的 HTML"**，剩下交给界面：
//! 界面在一个隐藏的 iframe 里载入它，调 `iframe.contentWindow.print()`。
//! 浏览器自己的打印面板里有"另存为 PDF"，对用户来说那就是"导出 PDF"。
//!
//! 代价要说清：**多了一步手动确认**（在打印面板里选"另存为 PDF"），
//! 程序无法替他决定。收益是排版由浏览器负责 —— 中文字体嵌入、分页、
//! 代码块跨页这些正是自己排最容易出错的地方。
//!
//! ## 与 HTML 导出的关系
//!
//! 用的是**同一段正文 HTML**（`crate::markdown::render_with`），
//! 只换外面那层壳：HTML 导出给屏幕（能点链接、能用系统配色），
//! 这里给纸（去掉深色配色、收窄、避免分页把代码拦腰截断）。

use crate::vault::export_html::escape_html;

/// 为**打印**准备的那层壳。
///
/// 与 [`crate::vault::export_html`] 的屏幕版是两份，因为两者要的东西相反：
/// 屏幕版跟着系统深浅主题走（`color-scheme: light dark`），
/// 打印版必须**固定成白底黑字** —— 否则深色主题下导出的 PDF 是一大块黑，
/// 打印出来还费墨。
fn print_style() -> &'static str {
    r#"
@page { margin: 16mm 14mm; }
html { color-scheme: light; }
body {
  margin: 0;
  padding: 0;
  font: 11pt/1.7 system-ui, -apple-system, "Noto Sans CJK SC",
        "Source Han Sans SC", "Microsoft YaHei", sans-serif;
  color: #1a1a1a;
  background: #fff;
  overflow-wrap: break-word;
}
h1 { font-size: 17pt; margin: 0 0 .8em; }
h2 { font-size: 14pt; } h3 { font-size: 12pt; }
h1, h2, h3, h4 { break-after: avoid; page-break-after: avoid; }
p, ul, ol, blockquote { margin: 0 0 .7em; }
ul, ol { padding-left: 1.4em; }
blockquote { margin-inline: 0; padding: .1em .9em; border-left: 3px solid #ccc; color: #444; }
code, pre, kbd { font-family: ui-monospace, "SF Mono", Consolas,
                 "Noto Sans Mono CJK SC", monospace; font-size: .92em; }
code { padding: .1em .3em; border-radius: 3px; background: #f2f2f2; }
pre {
  /* 长行折行而不是横向滚动：纸没有"滚"的概念，
     `overflow-x: auto` 在打印里只会把它截掉。 */
  overflow-x: visible;
  white-space: pre-wrap;
  padding: .7em .9em;
  border: 1px solid #ddd;
  border-radius: 4px;
  background: #fafafa;
  /* 整块挪到下一页，别在中间劈成两半 */
  break-inside: avoid;
  page-break-inside: avoid;
}
pre code { padding: 0; background: none; }
/* 代码块太长时实在避不开，就让它断，但别把每一行都留成孤儿 */
pre > code { orphans: 2; widows: 2; }
a { color: #1a1a1a; text-decoration: underline; }
img, svg, video { max-width: 100%; height: auto; break-inside: avoid; }
table { border-collapse: collapse; width: 100%; break-inside: avoid; page-break-inside: avoid; }
th, td { border: 1px solid #ccc; padding: .35em .6em; text-align: left; }
th { background: #f2f2f2; }
tr { break-inside: avoid; }
hr { border: 0; border-top: 1px solid #ddd; margin: 1.4em 0; }
mark { background: #ffe08a; color: inherit; }
img { border: none; }
"#
}

/// 给打印准备的那份完整 HTML（界面把它塞进隐藏 iframe 后调 `print()`）。
///
/// 与 [`crate::vault::export_html::wrap_document`] 的差别集中在"给纸看"三处：
/// 固定白底黑字、代码块折行而非横滚、以及分页保护（标题不落在页尾、
/// 代码块与表格不被劈开）。
pub fn wrap_for_print(title: &str, body_html: &str) -> String {
    let safe = escape_html(title);
    format!(
        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<title>{safe}</title>
<style>{PRINT_STYLE}</style>
</head>
<body>
{body_html}
</body>
</html>
"#,
        PRINT_STYLE = print_style()
    )
}

#[cfg(test)]
mod tests {
    use super::wrap_for_print;

    /// 三条"给纸看"的要求，缺一条 PDF 就会明显难看
    #[test]
    fn the_print_sheet_is_light_even_when_the_app_is_dark() {
        let html = wrap_for_print("t", "<p>x</p>");
        // 深色主题下导出的 PDF 是一大块黑 —— 所以必须钉死浅色
        assert!(html.contains("color-scheme: light"), "{html}");
        assert!(!html.contains("color-scheme: light dark"), "别跟着系统走");
        assert!(html.contains("background: #fff"), "底色要固定");
        assert!(html.contains("color: #1a1a1a"), "字色要固定");
    }

    #[test]
    fn long_code_wraps_instead_of_scrolling() {
        let html = wrap_for_print("t", "");
        // 纸没有"滚"的概念：overflow-x: auto 在打印时会把它截掉
        assert!(html.contains("white-space: pre-wrap"), "代码要折行");
        assert!(html.contains("overflow-x: visible"), "不能是 auto");
    }

    #[test]
    fn blocks_are_kept_off_page_breaks() {
        let html = wrap_for_print("t", "");
        // 标题落在页尾、代码块被劈成两半，是 PDF 最难看的两种
        assert!(html.contains("break-after: avoid"), "标题不该落在页尾");
        assert!(html.contains("page-break-after: avoid"), "旧引擎也要认");
        assert!(html.contains("break-inside: avoid"), "代码块/表格不该被劈开");
    }

    #[test]
    fn the_title_is_escaped_here_too() {
        let html = wrap_for_print("a<b>", "<p>x</p>");
        assert!(html.contains("<title>a&lt;b&gt;</title>"), "{html}");
        assert_eq!(html.matches("<title>").count(), 1);
    }
}
