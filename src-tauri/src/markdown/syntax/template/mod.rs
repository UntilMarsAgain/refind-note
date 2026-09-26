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
pub use stdlib::TEMPLATES;

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
        // ---- 块边界在这里定规则 ----
        //
        // 编辑器里有一份**逐条一致的镜像**（`NoteEditor.vue` 的 `templateBlockEnd`），
        // 用来在编辑时画出块的边界。改这里的规则要同时改那里，否则高亮会比渲染早收
        // 或晚收 —— 编辑器与渲染各说各话，正是这套对齐要消灭的东西。
        //
        // 三条规则：更深算块内；不更深即结束；**空行不直接结束**，要往后看一行 ——
        // 后面还有更深的内容，这个空行才算块内。
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

    /// 空行**不直接结束块**：后面还有更深的内容，它就算块内。
    ///
    /// 这条是编辑器那份镜像最容易走样的地方（"空行即结束"看着更直觉），
    /// 所以在这里钉死，改渲染规则时会先撞上它。
    #[test]
    fn blank_line_does_not_end_the_block_by_itself() {
        let html = render("::quote\n  第一段\n\n  第二段\n块外\n");
        assert!(html.contains("第一段"), "{html}");
        assert!(html.contains("第二段"), "{html}");
        // 两段都应当在同一个引用块里
        assert_eq!(html.matches("<blockquote").count(), 1, "{html}");
        let before = &html[..html.find("第二段").unwrap()];
        let depth =
            before.matches("<blockquote").count() - before.matches("</blockquote>").count();
        assert_eq!(depth, 1, "第二段应当仍在块内：{html}");
        // 不再是更深的那一行才结束
        assert!(html.contains("<p>块外</p>"), "{html}");
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
    fn code_template_carries_the_frontend_hints() {
        let html = render(
            "::code lang=rust lines=off start=10 highlight=2-3\n  fn main() {}\n  // 注释\n",
        );
        assert!(html.contains(r#"<pre class="template-code""#), "{html}");
        assert!(html.contains(r#"data-lines="off""#), "{html}");
        assert!(html.contains(r#"data-line-start="10""#), "{html}");
        assert!(html.contains(r#"data-highlight="2-3""#), "{html}");
        assert!(html.contains(r#"<code class="language-rust">"#), "{html}");
        assert!(html.contains("fn main() {}"), "{html}");
    }

    #[test]
    fn code_template_does_not_parse_markdown() {
        // 代码里的记号不该被当成 markdown（这是"像代码块"的关键）
        let html = render("::code\n  **不该变粗**\n");
        assert!(html.contains("**不该变粗**"), "{html}");
        assert!(!html.contains("<strong>"), "{html}");
    }

    #[test]
    fn aside_renders_its_body_as_markdown() {
        let html = render("::aside title=\"十四门桥\"\n  正文里的**强调**\n");
        assert!(html.contains(r#"<aside class="aside">"#), "{html}");
        assert!(html.contains("aside__title"), "{html}");
        assert!(
            html.contains("<strong>强调</strong>"),
            "信息栏的内容仍按 markdown 解析：{html}"
        );
    }

    #[test]
    fn fields_split_label_and_value() {
        let html = render("::fields\n  地址 | 福建省某村\n  分类 | 古建筑\n");
        assert!(html.contains(r#"<table class="fields">"#), "{html}");
        assert!(html.contains("<th>地址</th>"), "{html}");
        assert!(html.contains("<td>福建省某村</td>"), "{html}");

        // 没有 `|` 的行：整行当标签、值留空 —— 宁可少一栏，也不丢作者写下的字
        let one = render("::fields\n  只有标签\n");
        assert!(one.contains("<th>只有标签</th>"), "{one}");
    }

    #[test]
    fn banner_takes_the_body_or_the_text_param() {
        let html = render("::banner\n  福建省文物保护单位\n");
        assert!(
            html.contains(r#"<p class="banner">福建省文物保护单位</p>"#),
            "{html}"
        );
        let param = render("::banner text=另一种写法\n");
        assert!(param.contains("另一种写法"), "{param}");
        // 空内容要给提示，而不是画一条空带
        let empty = render("::banner\n");
        assert!(empty.contains("template--problem"), "{empty}");
    }

    #[test]
    fn image_carries_alignment_and_limits() {
        let html = render("::image src=/logo.svg align=right width=320 caption=\"桥体\"\n");
        assert!(html.contains(r#"<figure class="image image--right">"#), "{html}");
        assert!(html.contains(r#"src="/logo.svg""#), "{html}");
        assert!(html.contains("max-width: 320px"), "{html}");
        assert!(html.contains("<figcaption>桥体</figcaption>"), "{html}");
    }

    #[test]
    fn image_refuses_dangerous_sources_and_sneaky_sizes() {
        // 危险协议：不渲染图片，只给提示
        // （提示框里会**回显**参数原文，那是文本、不是属性 —— 所以断言针对"有没有 img"）
        let bad = render("::image src=javascript:alert(1)\n");
        assert!(!bad.contains("<img"), "危险协议不该渲染成图片：{bad}");
        assert!(bad.contains("template--problem"), "{bad}");

        // 尺寸只认"数字 + 可选单位"：塞别的声明一律不认，因此拼不出 style 属性
        let sneaky =
            render("::image src=/x.svg width=\"1px; background: url(//evil)\"\n");
        assert!(!sneaky.contains("style="), "尺寸参数不该拼出 style 属性：{sneaky}");

        // 没有 src：说清缺什么
        let missing = render("::image\n");
        assert!(missing.contains("template--problem"), "{missing}");
        assert!(missing.contains("src"), "{missing}");
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
