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

//! 把一段正文 HTML 包成**能单独打开的文件**。
//!
//! ## 为什么单独一个模块，而不是塞进渲染器
//!
//! 因为它服务的目标不同：渲染器（`markdown/mod.rs`）给的是**给程序自己看**的片段，
//! 要与阅读页、和编辑器预览长得一样；这里给的是**给人拿走看的文件**，要能脱离
//! 本程序、脱离网络、脱离仓库独立存在。两者共用同一套渲染结果（所以"导出看到的
//! 就是读到的"），但外面那层壳完全不同 —— 混进渲染器会让渲染器背上
//! "导出时要怎样"这种它不该关心的知识。

/// 转义成 HTML 文本（标题会进 `<title>` 与 `<h1>`）。
///
/// 自己写而不是靠某个 crate：这里只有三个字符要做，而且这层壳刻意不引依赖
/// ——它要被塞进每一个导出的文件里。
pub fn escape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            other => out.push(other),
        }
    }
    out
}

/// 随文件带出去的样式。
///
/// 刻意**只覆盖"没有它就读不下去"的那些**，不复制 `styles/` 全套：
/// 那个是给屏幕看的（深浅主题、主题色、宽窄限制），搬过来会让导出文件在
/// 别人机器上带着别人的配色。而导出要的是"拿到就能读、字距行距正常、
/// 代码块能横向滚、表格不撑破页面"。
const STYLE: &str = r#"
:root { color-scheme: light dark; }
body {
  margin: 0 auto;
  padding: 2.5rem 1.25rem 6rem;
  max-width: 44rem;
  /* 不写死颜色：跟系统走，深浅主题下都读得清。
     真的想固定配色，用 print 时加的那段。 */
  font: 16px/1.75 system-ui, -apple-system, "Segoe UI", "Noto Sans CJK SC",
        "Source Han Sans SC", "Microsoft YaHei", sans-serif;
  color: CanvasText;
  background: Canvas;
  overflow-wrap: break-word;
}
h1, h2, h3, h4, h5, h6 { line-height: 1.35; margin: 1.6em 0 .6em; }
h1 { font-size: 1.7em; } h2 { font-size: 1.4em; } h3 { font-size: 1.15em; }
p, ul, ol, blockquote, table, pre { margin: 0 0 1em; }
ul, ol { padding-left: 1.6em; }
blockquote {
  margin-inline: 0;
  padding: .1em 1em;
  border-left: 3px solid color-mix(in srgb, CanvasText 25%, transparent);
  color: color-mix(in srgb, CanvasText 78%, Canvas);
}
code, pre, kbd, samp {
  font-family: ui-monospace, "SF Mono", "Cascadia Code", Consolas,
               "Noto Sans Mono CJK SC", monospace;
  font-size: .92em;
}
code { padding: .12em .35em; border-radius: 4px;
       background: color-mix(in srgb, CanvasText 9%, Canvas); }
pre {
  /* 关键：长行必须能横向滚，而不是把整页撑宽 —— 一行代码撑破页面，
     在手机上尤其难看。 */
  overflow-x: auto;
  padding: .85em 1em;
  border-radius: 6px;
  background: color-mix(in srgb, CanvasText 7%, Canvas);
}
pre code { padding: 0; background: none; }
a { color: color-mix(in srgb, CanvasText 25%, LinkText); }
img, video { max-width: 100%; height: auto; }
table { border-collapse: collapse; display: block; overflow-x: auto; max-width: 100%; }
th, td { border: 1px solid color-mix(in srgb, CanvasText 25%, transparent);
         padding: .4em .7em; text-align: left; }
th { background: color-mix(in srgb, CanvasText 6%, Canvas); }
hr { border: 0; border-top: 1px solid color-mix(in srgb, CanvasText 20%, transparent); }
mark { background: color-mix(in srgb, #ffe08a 60%, Canvas); color: inherit; }
details > summary { cursor: pointer; }

/* 打印时收窄一点、去掉留白 —— 纸比屏幕窄 */
@media print {
  body { max-width: none; padding: 0; font-size: 11.5pt; }
  pre, table { overflow: visible; }
  a { color: inherit; text-decoration: underline; }
}
"#;

/// 把正文片段包成一份完整的 HTML 文档。
///
/// `title` 放进 `<title>` 与文档开头的 `<h1>`：导出的文件要**自己说清是什么**，
/// 而不是靠文件名——文件名常常被改过。
pub fn wrap_document(title: &str, body_html: &str) -> String {
    let safe = escape_html(title);
    format!(
        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{safe}</title>
<style>{STYLE}</style>
</head>
<body>
<h1>{safe}</h1>
{body_html}
</body>
</html>
"#
    )
}

#[cfg(test)]
mod tests {
    use super::{escape_html, wrap_document};

    #[test]
    fn the_document_is_complete_enough_to_open_on_its_own() {
        let html = wrap_document("测试笔记", "<p>正文</p>");
        // 这四样缺一个，"双击打开"就不成立
        assert!(html.contains("<!DOCTYPE html>"), "缺 doctype：浏览器会进兼容模式");
        assert!(html.contains(r#"<meta charset="utf-8">"#), "缺 charset：中文会乱码");
        assert!(html.contains("<title>测试笔记</title>"), "缺 title");
        assert!(html.contains("<style>"), "缺样式：裸标签很难读");
        assert!(html.contains("<h1>测试笔记</h1>"), "正文前应有标题");
        assert!(html.contains("<p>正文</p>"), "正文本身要原样进去");
    }

    #[test]
    fn the_title_cannot_break_out_of_its_tag() {
        // 标题里的尖括号会闭合掉标签，整份文档的骨架就塌了
        let html = wrap_document("a<b>c", "<p>x</p>");
        assert!(html.contains("<title>a&lt;b&gt;c</title>"), "{html}");
        assert!(html.contains("<h1>a&lt;b&gt;c</h1>"), "{html}");
        // 不能有真的 </title> 提前闭合
        assert_eq!(html.matches("<title>").count(), 1);
    }

    #[test]
    fn ampersands_are_escaped_too() {
        assert_eq!(escape_html("a & b"), "a &amp; b");
        assert_eq!(escape_html("<>&\""), "&lt;&gt;&amp;\"");
        assert_eq!(escape_html("没有特殊字符"), "没有特殊字符");
    }

    #[test]
    fn long_code_lines_scroll_instead_of_breaking_the_page() {
        // 一行长代码把整页撑宽是导出 HTML 最难看的问题
        let style = wrap_document("t", "<p>x</p>");
        assert!(style.contains("overflow-x: auto"), "pre 必须能横向滚");
        assert!(style.contains("pre-wrap") || style.contains("overflow-wrap"), "长单词要能断行");
    }

    #[test]
    fn printing_drops_the_screen_padding() {
        assert!(wrap_document("t", "").contains("@media print"), "应带打印样式");
    }
}
