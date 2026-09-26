//! 模板块：`::名字 参数=值 …` 开一个块，块的边界由**缩进**划出。
//!
//! ```text
//! ::name key=value key=value
//!   content
//! ```
//!
//! 三个子模块各管一段，本文件只管**认出块**：
//!
//! - [`parse`]：头解析（引号、转义）—— 这里能写的东西最多，所以单独一个文件；
//! - [`fill`]：`{{}}` 填空与 HTML / CSS 过滤；
//! - [`dispatch`]：按名字分发，查不到就渲染"未知模板"的框；
//! - [`stdlib`]：模板标准库（`quote`），加模板只改那一个文件。
//!
//! 块内容的边界规则只有一条：**缩进**。头行之后缩进更深的行属于块内，遇到不更深的行
//! 就结束，不需要结束标记。
mod dispatch;
mod fill;
mod parse;
mod stdlib;

pub use parse::Template;

use markdown_it::parser::block::{BlockRule, BlockState};
use markdown_it::{MarkdownIt, Node, NodeValue, Renderer};

impl NodeValue for Template {
    fn render(&self, node: &Node, fmt: &mut dyn Renderer) {
        dispatch::render(self, node, fmt);
    }
}

/// 拼回块内容：`(相对块头的缩进, 行文本)` → 去掉公共缩进后的文本。
///
/// **必须用 `state.line_indent` 的数值**，不能去数字符串开头的空格：
/// markdown-it 的 `get_line` 已经把行首空白吃掉了（"trimming initial spaces"），
/// 数出来永远是 0 —— 那样块内的层级会被悄悄抹平，嵌套的模板就坏了。
fn rebuild_body(lines: &[(usize, &str)]) -> String {
    let shared = lines
        .iter()
        .filter(|(_, text)| !text.trim().is_empty())
        .map(|(indent, _)| *indent)
        .min()
        .unwrap_or(0);
    lines
        .iter()
        .map(|(indent, text)| {
            if text.is_empty() {
                String::new()
            } else {
                format!("{}{}", " ".repeat(indent.saturating_sub(shared)), text)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn add(md: &mut MarkdownIt) {
    // 必须排在段落规则之前：否则 `::名字` 会先被当成一行普通文字收走
    md.block
        .add_rule::<TemplateScanner>()
        .before::<markdown_it::plugins::cmark::block::paragraph::ParagraphScanner>()
        .after_all();
}

#[doc(hidden)]
pub struct TemplateScanner;

impl BlockRule for TemplateScanner {
    fn run(state: &mut BlockState) -> Option<(Node, usize)> {
        let start = state.line;
        let marker_indent = state.line_indent(start);
        if marker_indent < 0 {
            return None;
        }
        let (name, params) = Template::parse_header(state.get_line(start))?;

        // (相对块头的缩进, 行文本)：缩进要留着，块内容的层级靠它
        let mut body_lines: Vec<(usize, &str)> = Vec::new();
        let mut line = start + 1;
        while line < state.line_max {
            // 空行的 `line_indent` 是 0 而不是 -1，必须用 `is_empty` 单独判
            let blank = state.is_empty(line) || state.line_indent(line) < 0;
            if blank {
                // 空行：只有后面还有更深的内容时才算块内，否则它属于块外
                let next = (line + 1..state.line_max)
                    .find(|candidate| !state.is_empty(*candidate) && state.line_indent(*candidate) >= 0);
                match next {
                    Some(next) if state.line_indent(next) > marker_indent => {
                        body_lines.push((0, ""));
                        line += 1;
                        continue;
                    }
                    _ => break,
                }
            }
            if state.line_indent(line) <= marker_indent {
                break;
            }
            let indent = state.line_indent(line).max(0) as usize;
            body_lines.push((indent, state.get_line(line)));
            line += 1;
        }

        let body = rebuild_body(&body_lines);
        let mut node = Node::new(Template {
            name,
            params,
            body: body.clone(),
        });
        // 块内容交给**整个解析器**再解析一遍：模板里因此可以写 markdown、内部链接，
        // 也可以再嵌模板（嵌套的 `::quote` 就是靠这一步成立的）。
        //
        // `Node` 带 Drop，字段不能直接搬出来，所以用 `mem::take` 换走它的 children。
        let mut parsed = state.md.parse(&body);
        node.children = std::mem::take(&mut parsed.children);
        Some((node, line - start))
    }
}

#[cfg(test)]
mod tests {
    use crate::markdown::render;

    #[test]
    fn indentation_separates_inside_from_outside() {
        let html = render("::note\n  块内\n块外\n");
        assert!(html.contains("块内"), "{html}");
        assert!(html.contains("<p>块外</p>"), "{html}");
    }

    #[test]
    fn deeper_levels_stay_inside() {
        let html = render("::note\n  第一层\n    ::inner\n      第二层\n");
        assert!(html.contains("::inner"), "{html}");
        assert!(html.contains("第二层"), "{html}");
    }

    #[test]
    fn blank_line_inside_stays_inside() {
        let html = render("::note\n  第一段\n\n  第二段\n");
        assert!(html.contains("第一段"), "{html}");
        assert!(html.contains("第二段"), "{html}");
        assert!(!html.contains("<p>第二段</p>"), "第二段仍应在块内：{html}");
    }

    #[test]
    fn blank_line_before_unindented_text_ends_the_block() {
        let html = render("::note\n  块内\n\n块外\n");
        assert!(html.contains("块内"), "{html}");
        assert!(html.contains("<p>块外</p>"), "{html}");
    }

    #[test]
    fn dedent_keeps_relative_structure() {
        let html = render("::note\n    两空格缩进之下\n      再深一层\n");
        assert!(html.contains("两空格缩进之下"), "{html}");
        assert!(html.contains("再深一层"), "{html}");
    }

    #[test]
    fn body_is_parsed_by_the_whole_parser() {
        // 模板内容里的 markdown 与内部链接都要生效
        let html = render("::quote\n  这是**强调**与[[目标]]\n");
        assert!(html.contains("<strong>强调</strong>"), "{html}");
        assert!(html.contains("wikilink"), "{html}");
    }

    #[test]
    fn quote_template_renders_with_origin() {
        let html = render("::quote origin=\"某 人\"\n  引用内容\n");
        assert!(html.contains(r#"<blockquote class="quote">"#), "{html}");
        assert!(html.contains("引用内容"), "{html}");
        assert!(html.contains("quote__origin"), "{html}");
        // 署名带一条横线，且引号保护的空格原样保留
        assert!(html.contains("— 某 人"), "{html}");
    }

    #[test]
    fn quote_without_origin_has_no_signature() {
        let html = render("::quote\n  只引用\n");
        assert!(html.contains("<blockquote"), "{html}");
        assert!(!html.contains("quote__origin"), "{html}");
    }

    #[test]
    fn quote_nests() {
        let html = render("::quote\n  ::quote origin=\"内层\"\n    文字\n");
        assert_eq!(html.matches("<blockquote").count(), 2, "{html}");
        assert!(html.contains("内层"), "{html}");

        // 内容必须落在**内层**里：数一数"文字"之前有几个未闭合的 blockquote
        // （1 = 只在外层，2 = 正确地嵌在内层）。用字符串顺序猜结构是不可靠的。
        let before = &html[..html.find("文字").expect("应当渲染出内容")];
        let depth =
            before.matches("<blockquote").count() - before.matches("</blockquote>").count();
        assert_eq!(depth, 2, "内容应当嵌在内层 blockquote 里：{html}");
    }

    #[test]
    fn css_template_injects_style_with_our_variables() {
        // 注入的 CSS 与界面同一个文档，所以 var(--accent) 这类变量直接用
        let html = render("::css\n  .我有自己的样式 { color: var(--accent); }\n");
        assert!(html.contains("<style>"), "{html}");
        assert!(html.contains("var(--accent)"), "{html}");
    }

    #[test]
    fn css_cannot_close_the_style_block_early() {
        let html = render("::css\n  a { } </style><p>顶出来了</p>\n");
        assert_eq!(html.matches("</style>").count(), 1, "{html}");
        assert!(html.contains("<p>顶出来了</p>"), "它仍然只是普通正文：{html}");
    }

    #[test]
    fn html_template_filters_scripts_by_default() {
        let html = render("::html\n  <b>粗</b><script>alert(1)</script>\n");
        assert!(html.contains("<b>粗</b>"), "{html}");
        assert!(!html.contains("<script"), "默认必须过滤掉脚本：{html}");
    }

    #[test]
    fn html_template_allows_scripts_only_when_asked() {
        let html = render("::html js\n  <script>alert(1)</script>\n");
        assert!(html.contains("<script>alert(1)</script>"), "{html}");
    }

    #[test]
    fn placeholders_are_filled_from_params() {
        let html = render("::css 颜色=red\n  .a { color: {{颜色}}; }\n");
        assert!(html.contains("color: red"), "{html}");
    }

    #[test]
    fn unknown_template_renders_a_box() {
        let html = render("::还没有的模板 标题=\"含 空格\" flag\n  内容一行\n");
        assert!(html.contains("template--unknown"), "{html}");
        assert!(html.contains("未知模板"), "{html}");
        assert!(
            html.contains(r#"<code class="template__name">还没有的模板</code>"#),
            "{html}"
        );
        assert!(html.contains("标题=含 空格"), "{html}");
        assert!(html.contains("<pre><code>内容一行</code></pre>"), "{html}");
    }

}
