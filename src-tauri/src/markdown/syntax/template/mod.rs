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

// 标准模板表对外公开：模板分发与调试都要按名字看这张表。
#[allow(unused_imports)]
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
        // 三条规则：更深算块内；不更深即结束；**空行不直接结束**，要往后看一行 ——
        // 后面还有更深的内容，这个空行才算块内。
        let mut body_lines: Vec<(usize, &str)> = Vec::new();
        let mut line = start + 1;
        while line < state.line_max {
            // 空行的 `line_indent` 是 0 而不是 -1，必须用 `is_empty` 单独判
            let blank = state.is_empty(line) || state.line_indent(line) < 0;
            if blank {
                // 空行：只有后面还有更深的内容时才算块内，否则它属于块外
                let next = (line + 1..state.line_max).find(|candidate| {
                    !state.is_empty(*candidate) && state.line_indent(*candidate) >= 0
                });
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
        // `text=…` 给了就以它为准（`::banner` 那种一条横条，正文与参数二选一）。
        // 它也要能写 markdown，所以和块内容走同一条路。
        let source = match params.iter().find(|(key, _)| key == "text") {
            Some((_, text)) => text.clone(),
            None => body.clone(),
        };

        let sectioned = stdlib::takes_sections(&name);
        let mut node = Node::new(Template { name, params, body });
        // 内容交给**整个解析器**再解析一遍：模板里因此可以写 markdown、内部链接，
        // 也可以再嵌模板（嵌套的 `::quote` 就是靠这一步成立的）。
        //
        // `Node` 带 Drop，字段不能直接搬出来，所以用 `mem::take` 换走它的 children。
        if sectioned {
            // 分节模板：块内容先按 `[标签]` 切成几节，**各节各解析一遍** ——
            // 一节是一篇小文档，这才谈得上"这一节里写什么"（见 [`Section`]）
            for (label, text) in split_sections(&source) {
                let mut section = Node::new(Section { label });
                let mut parsed = state.md.parse(&text);
                section.children = std::mem::take(&mut parsed.children);
                node.children.push(section);
            }
        } else {
            let mut parsed = state.md.parse(&source);
            node.children = std::mem::take(&mut parsed.children);
        }
        Some((node, line - start))
    }
}

/// 把块内容按 `[标签]` 行切成几节：`(标签, 该节的正文)`。
///
/// 第一个标签**之前**的内容归到一个标签为空串的节里 —— 切分只管切，
/// 那一节算"两边都要"还是"这写法不对"，由各个模板自己定。
fn split_sections(source: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for line in source.lines() {
        if let Some(label) = section_label(line) {
            out.push((label, String::new()));
            continue;
        }
        if out.is_empty() {
            // 还没有任何标签：空白丢掉，有字就先开一节（空标签）
            if line.trim().is_empty() {
                continue;
            }
            out.push((String::new(), String::new()));
        }
        let section = out.last_mut().expect("上面保证过至少开了一节");
        // 节首的空行不留（那是标签与正文之间的空档）；节内的空行留着，正文要用它分段
        if line.trim().is_empty() && section.1.trim().is_empty() {
            continue;
        }
        section.1.push_str(line);
        section.1.push('\n');
    }
    out
}

/// `[标签]` —— 分节模板里一节的开头。
///
/// 两条都认：**整行**就是一对中括号（正文里一句 `[注] 说明` 不该被当成新的一节），
/// 而且**从行首开始**（块内容此时已去掉公共缩进，所以本体的一行就在第 0 列，
/// 缩进更深的是**里层**的内容）。少了第二条，嵌套的选项卡会被外层吃掉：
/// 里层的 `[内]` 也当成外层的节了。
fn section_label(line: &str) -> Option<String> {
    let inner = line.strip_prefix('[')?.trim_end().strip_suffix(']')?;
    let label = inner.trim();
    if label.is_empty() || label.contains(['[', ']']) {
        return None;
    }
    Some(label.to_string())
}

#[cfg(test)]
mod tests {
    use crate::markdown::render;

    /// 标题带里的内容按 markdown 渲染：加粗、内部链接都算数
    #[test]
    fn banner_renders_markdown_in_its_body() {
        let html = render("::banner\n  这是**加粗**与[[目标]]\n");
        assert!(html.contains("<strong>加粗</strong>"), "{html}");
        assert!(html.contains("wikilink"), "{html}");
        // 一条横条：不该在横条里再套一个段落
        assert!(!html.contains("<p><p>"), "{html}");
        assert_eq!(html.matches("class=\"banner\"").count(), 1, "{html}");
    }

    /// `text=` 同样按 markdown 渲染
    #[test]
    fn banner_renders_markdown_in_its_text_parameter() {
        let html = render("::banner text=\"**加粗**的标题\"\n");
        assert!(html.contains("<strong>加粗</strong>"), "{html}");
    }

    /// `::title`：居中的大标题，没有背景；`color=` 给的是字色
    #[test]
    fn title_renders_a_centered_heading_without_a_background() {
        let html = render("::title\n  卷首的题名\n");
        assert!(html.contains("class=\"title-block\""), "{html}");
        assert!(html.contains("卷首的题名"), "{html}");
        assert!(!html.contains("background"), "大标题没有背景：{html}");

        let colored = render("::title text=\"**重**的标题\" color=#123456\n");
        assert!(colored.contains("color: #123456"), "{colored}");
        assert!(colored.contains("<strong>重</strong>"), "{colored}");

        let bad = render("::title text=\"x\" color=红色\n");
        assert!(bad.contains("template--problem"), "{bad}");
    }

    /// `::banner` 的高度与圆角：高度是**最小**高度，圆角默认开着
    #[test]
    fn banner_takes_a_height_and_a_square_corner() {
        let sized = render("::banner text=\"标题\" height=120px\n");
        assert!(sized.contains("min-height: 120px"), "{sized}");
        assert!(sized.contains("banner--sized"), "{sized}");
        assert!(
            !sized.contains("banner--square"),
            "不写 rounded= 就是圆角：{sized}"
        );

        let square = render("::banner text=\"标题\" rounded=off\n");
        assert!(square.contains("banner--square"), "{square}");

        let bad = render("::banner text=\"标题\" height=很久\n");
        assert!(bad.contains("template--problem"), "{bad}");
        let bad = render("::banner text=\"标题\" rounded=也许\n");
        assert!(bad.contains("template--problem"), "{bad}");
    }

    /// `::video` / `::audio`：摆一个带控件的播放器（源写的是仓库里的名字）
    #[test]
    fn video_and_audio_render_players() {
        let video = render("::video src=片子.mp4 width=720px\n  一段注释\n");
        assert!(video.contains("<video"), "{video}");
        assert!(video.contains("controls"), "{video}");
        assert!(video.contains("src=\"片子.mp4\""), "{video}");
        assert!(video.contains("max-width: 720px"), "{video}");
        assert!(
            video.contains("<figcaption>一段注释</figcaption>"),
            "{video}"
        );

        let audio = render("::audio src=录音.mp3\n");
        assert!(audio.contains("<audio"), "{audio}");
        assert!(audio.contains("controls"), "{audio}");

        // 没有 src 就报用法问题，而不是摆一个空的播放器
        let bare = render("::video\n");
        assert!(bare.contains("template--problem"), "{bare}");
    }

    /// `::theme`：两节都渲染出来，由样式按主题决定显示哪一节
    #[test]
    fn theme_renders_both_versions_and_lets_the_stylesheet_pick() {
        let html = render("::theme\n  [light]\n  浅色下看这个\n  [dark]\n  深色下看这个\n");
        assert!(html.contains("theme__part--light"), "{html}");
        assert!(html.contains("theme__part--dark"), "{html}");
        assert!(
            html.contains("浅色下看这个") && html.contains("深色下看这个"),
            "{html}"
        );

        // 标签之前的内容两边都显示
        let both = render("::theme\n  两种主题都显示\n  [dark]\n  只有深色\n");
        assert!(both.contains("class=\"theme__part\""), "{both}");

        let bad = render("::theme\n  [blue]\n  没有这个主题\n");
        assert!(bad.contains("template--problem"), "{bad}");
    }

    /// `::tabs`：后端给结构与内容（第一节选中），切换留给前端
    #[test]
    fn tabs_render_a_bar_and_one_panel_each() {
        let html = render("::tabs\n  [北岸]\n  走北路\n  [南岸]\n  走南路\n");
        assert_eq!(html.matches("class=\"tabs__tab").count(), 2, "{html}");
        assert!(html.contains("tabs__tab--on"), "{html}");
        assert!(html.contains("走北路") && html.contains("走南路"), "{html}");
        // 没选中的面板先藏着，点标签才出来（前端接线）
        assert!(html.contains("hidden"), "{html}");

        // 一节里的正文按 markdown 渲染
        let rich = render("::tabs\n  [一]\n  **加粗**\n");
        assert!(rich.contains("<strong>加粗</strong>"), "{rich}");

        let stray = render("::tabs\n  没写标签的内容\n  [一]\n  正文\n");
        assert!(stray.contains("template--problem"), "{stray}");

        // 一节里还能再嵌一个选项卡（各节是各自解析的小文档）
        let nested = render("::tabs\n  [甲]\n  ::tabs\n    [内]\n    内文\n  [乙]\n  乙文\n");
        assert_eq!(nested.matches("class=\"tabs\"").count(), 2, "{nested}");
        assert!(
            nested.contains("内文") && nested.contains("乙文"),
            "{nested}"
        );
    }

    /// `::js`：内容原样进 <script>，只是 `</script` 会被转义掉
    #[test]
    fn js_goes_in_raw_and_cannot_close_its_own_tag() {
        let html = render("::js\n  console.log(\"你好\");\n");
        assert!(html.contains("<script"), "{html}");
        assert!(html.contains("console.log(\"你好\");"), "{html}");

        let tricky = render("::js\n  const s = \"</script>\";\n");
        assert!(
            !tricky.contains("</script>\";"),
            "不能让它提前闭合：{tricky}"
        );
        assert!(tricky.contains("<\\/script>"), "{tricky}");

        // 大小写照原样留着：那可能是字符串里的内容，改一个字母就改了它的意思
        let shouted = render("::js\n  const s = \"</SCRIPT>\";\n");
        assert!(shouted.contains("<\\/SCRIPT>"), "{shouted}");
    }

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
    /// 这条是块边界规则里最容易走样的地方（"空行即结束"看着更直觉），
    /// 所以在这里钉死，改渲染规则时会先撞上它。
    #[test]
    fn blank_line_does_not_end_the_block_by_itself() {
        let html = render("::quote\n  第一段\n\n  第二段\n块外\n");
        assert!(html.contains("第一段"), "{html}");
        assert!(html.contains("第二段"), "{html}");
        // 两段都应当在同一个引用块里
        assert_eq!(html.matches("<blockquote").count(), 1, "{html}");
        let before = &html[..html.find("第二段").unwrap()];
        let depth = before.matches("<blockquote").count() - before.matches("</blockquote>").count();
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
        let depth = before.matches("<blockquote").count() - before.matches("</blockquote>").count();
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
        assert!(
            html.contains("<p>顶出来了</p>"),
            "它仍然只是普通正文：{html}"
        );
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
    fn banner_color_sets_background_and_readable_text() {
        // 深蓝底 → 浅色字
        let dark = render("::banner color=#0055a4\n  深蓝底\n");
        assert!(dark.contains("background: #0055a4"), "{dark}");
        assert!(dark.contains("color: #f5f5f5"), "{dark}");

        // 浅黄底 → 深色字（深底配深字是最常见的自挖坑）
        let light = render("::banner color=#ffe680\n  浅黄底\n");
        assert!(light.contains("color: #101010"), "{light}");

        // `#abc` 这种简写也认，并规范成六位
        let short = render("::banner color=#0af\n  简写\n");
        assert!(short.contains("background: #00aaff"), "{short}");

        // 乱写的颜色：不猜，直接说清楚（值会进 style 属性，不能宽松）
        let bad = render("::banner color=red\n  乱写\n");
        assert!(bad.contains("template--problem"), "{bad}");
        assert!(!bad.contains("style="), "不该拼出 style 属性：{bad}");
    }

    #[test]
    fn image_carries_alignment_and_limits() {
        let html = render("::image src=/logo.svg align=right width=320 caption=\"桥体\"\n");
        assert!(
            html.contains(r#"<figure class="image image--right">"#),
            "{html}"
        );
        assert!(html.contains(r#"src="/logo.svg""#), "{html}");
        assert!(html.contains("max-width: 320px"), "{html}");
        assert!(html.contains("<figcaption>桥体</figcaption>"), "{html}");
    }

    #[test]
    fn image_takes_its_caption_from_the_body() {
        // 图片下面写一行 —— 最自然的写法
        let html = render("::image src=/logo.svg\n  桥体（一〇七九年）\n");
        assert!(
            html.contains("<figcaption>桥体（一〇七九年）</figcaption>"),
            "{html}"
        );

        // 块内容在块里是被缩进的，注释不该带着那些空格
        let indented = render("::image src=/logo.svg\n    两边都有空格\n");
        assert!(
            indented.contains("<figcaption>两边都有空格</figcaption>"),
            "{indented}"
        );

        // 多行拼成一句
        let multiline = render("::image src=/logo.svg\n  第一行\n  第二行\n");
        assert!(
            multiline.contains("<figcaption>第一行 第二行</figcaption>"),
            "{multiline}"
        );

        // `caption=` 优先于块内容
        let explicit = render("::image src=/logo.svg caption=显式\n  块里那句\n");
        assert!(
            explicit.contains("<figcaption>显式</figcaption>"),
            "{explicit}"
        );
        assert!(!explicit.contains("块里那句"), "{explicit}");

        // 都没有：不出现空的图注
        let none = render("::image src=/logo.svg\n");
        assert!(!none.contains("<figcaption>"), "{none}");
    }

    #[test]
    fn image_refuses_dangerous_sources_and_sneaky_sizes() {
        // 危险协议：不渲染图片，只给提示
        // （提示框里会**回显**参数原文，那是文本、不是属性 —— 所以断言针对"有没有 img"）
        let bad = render("::image src=javascript:alert(1)\n");
        assert!(!bad.contains("<img"), "危险协议不该渲染成图片：{bad}");
        assert!(bad.contains("template--problem"), "{bad}");

        // 尺寸只认"数字 + 可选单位"：塞别的声明一律不认，因此拼不出 style 属性
        let sneaky = render("::image src=/x.svg width=\"1px; background: url(//evil)\"\n");
        assert!(
            !sneaky.contains("style="),
            "尺寸参数不该拼出 style 属性：{sneaky}"
        );

        // 没有 src：说清缺什么
        let missing = render("::image\n");
        assert!(missing.contains("template--problem"), "{missing}");
        assert!(missing.contains("src"), "{missing}");
    }

    /// `::mermaid`：原文照收（图定义里的记号不该被 markdown 解析），
    /// 另存一份在 `data-source` 上给"切主题重画"用
    #[test]
    fn mermaid_keeps_its_source_verbatim() {
        let html = render("::mermaid\n  graph TD\n    A[开始] --> B{行不行}\n     B -->|行| C[干活]\n");
        assert!(html.contains(r#"<pre class="mermaid""#), "{html}");
        assert!(html.contains("A[开始] --&gt; B{行不行}"), "箭头照原样：{html}");
        assert!(html.contains("data-source="), "原文要留一份：{html}");
        // 图定义里的 `-->` `{}` 不该变成 HTML 标签
        assert!(!html.contains("<b>"), "{html}");
    }

    #[test]
    fn mermaid_without_content_says_so() {
        let html = render("::mermaid\n");
        assert!(html.contains("template--problem"), "{html}");
    }

    /// `::math`：TeX 原文进 `data-tex`，元素里留着原文当兜底
    #[test]
    fn math_template_hands_the_source_to_the_frontend() {
        let html = render("::math\n  \\frac{a}{b} = c_1 * d\n");
        assert!(html.contains("math--display"), "{html}");
        assert!(html.contains(r#"data-tex="\frac{a}{b} = c_1 * d""#), "{html}");
        // `*` 与 `_` 是公式的一部分，不该被当成强调
        assert!(!html.contains("<em>"), "{html}");
    }

    /// `::signature`：落款 —— 内容按 markdown 走，右对齐交给样式
    #[test]
    fn signature_keeps_markdown_and_a_right_aligned_box() {
        let html = render("::signature\n  甲  \n  2026 年秋\n");
        assert!(html.contains(r#"<div class="signature">"#), "{html}");
        assert!(html.contains("甲"), "{html}");
        assert!(html.contains("2026 年秋"), "{html}");
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
