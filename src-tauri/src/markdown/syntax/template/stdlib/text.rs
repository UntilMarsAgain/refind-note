//! 只写文字的块：`quote` / `aside` / `banner` / `title` / `signature` / `fields`。
//!
//! 这一组的共同点是：**内容照常按 markdown 解析** —— 内容已经由整个解析器解析成子节点挂
//! 在 `node.children` 上，渲染器只管 `fmt.contents(&node.children)`，所以模板里能写链接、
//! 列表、内部链接，也能再嵌一个 `::` 块。要"不解析"的块在 [`super::embeds`]。
//!
//! 参数只管**外框**长什么样：位置（`align`）、颜色（`color=`）、高度（`height=`）、
//! 圆角。内容本身的规矩不在这里管。
//!
//! 组内还共享两样小工具，都写在下面渲染器的后面：
//!
//! - [`normalize_hex`] / [`text_on`]：颜色参数只认十六进制，且**字色按底色亮度定** ——
//!   深底配深字是最常见的"自己给自己挖坑"；
//! - [`inline_or_blocks`]："一条横条 / 一个大标题"里，内容该按行内还是按块渲染。

use super::super::dispatch::render_problem;
use super::super::parse::Template;
// 颜色那两个工具现在住在 stdlib 那一层：`color=` 有两组模板认它
use super::{normalize_hex, size_rule, text_on};
use markdown_it::plugins::cmark::block::paragraph::Paragraph;
use markdown_it::{Node, Renderer};

/// 署名前面那条横线。
///
/// 单独拎出来是因为它只该出现在一个地方：想换成两个破折号、或者换成普通连字符，
/// 改这里一处即可。
const ORIGIN_DASH: &str = "—";

/// `::quote origin="署名"` —— 效果等同 markdown 的 `>`，但可以在参数里给一个署名，
/// 渲染到右下角。
///
/// 内容由整个解析器解析过（见 `mod.rs`），所以里面可以写 markdown，也可以再嵌 `::quote`。
pub(super) fn render_quote(template: &Template, node: &Node, fmt: &mut dyn Renderer) {
    fmt.cr();
    // 用 blockquote 标签：左边那条竖线与缩进由正文样式统一负责，这里只补署名
    fmt.open("blockquote", &[("class", "quote".to_string())]);
    fmt.cr();
    fmt.contents(&node.children);
    if let Some(origin) = template.param("origin") {
        fmt.cr();
        fmt.open("p", &[("class", "quote__origin".to_string())]);
        fmt.text(ORIGIN_DASH);
        fmt.text(" ");
        fmt.text(origin);
        fmt.close("p");
    }
    fmt.cr();
    fmt.close("blockquote");
    fmt.cr();
}

/// `::aside title="十四门桥"` —— 右侧的信息栏，**内容照常按 markdown 渲染**。
///
/// 用 `node.children`（已经解析好的内容）而不是原文：信息栏里也要能写链接、强调、列表。
/// 它是一条**浮动**的栏：正文会绕着它走，这是信息栏该有的样子；窄屏上由样式取消浮动。
pub(super) fn render_aside(template: &Template, node: &Node, fmt: &mut dyn Renderer) {
    fmt.cr();
    fmt.open("aside", &[("class", "aside".to_string())]);
    if let Some(title) = template.param("title") {
        fmt.open("p", &[("class", "aside__title".to_string())]);
        fmt.text(title);
        fmt.close("p");
    }
    fmt.cr();
    fmt.contents(&node.children);
    fmt.cr();
    fmt.close("aside");
    fmt.cr();
}

/// `::fields` —— 左右两栏的属性表：每行 `标签 | 值`。
///
/// 这一栏是本组里唯一的例外：**值用的是原文**（不解析 markdown）—— 它是"查参数"用的，
/// 一行一项最清楚。没有 `|` 的行把整行当标签、值留空 —— 宁可少一栏，也不丢作者写下的字。
pub(super) fn render_fields(template: &Template, _node: &Node, fmt: &mut dyn Renderer) {
    fmt.cr();
    fmt.open("table", &[("class", "fields".to_string())]);
    fmt.open("tbody", &[]);
    for line in template.body.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let (label, value) = match line.split_once('|') {
            Some((label, value)) => (label.trim(), value.trim()),
            None => (line, ""),
        };
        fmt.cr();
        fmt.open("tr", &[]);
        fmt.open("th", &[]);
        fmt.text(label);
        fmt.close("th");
        fmt.open("td", &[]);
        fmt.text(value);
        fmt.close("td");
        fmt.close("tr");
    }
    fmt.cr();
    fmt.close("tbody");
    fmt.close("table");
    fmt.cr();
}

/// `::banner color=#0055a4` —— 一条方框标题带（信息栏里那种居中的标题条）。
///
/// 文案来自块内容（一行），或由 `text=` 给出；两者**都按 markdown 渲染**，
/// 所以加粗、链接、内部链接在这里同样有效。
/// `color=` 给底色（只认 `#rgb` / `#rrggbb`），字色由 [`text_on`] 按亮度定，
/// 免得深底配深字。
/// 名字取 `banner` 而不是"方框标题"之类：它是一个**横条**，越短越不容易与别的模板混淆。
pub(super) fn render_banner(template: &Template, node: &Node, fmt: &mut dyn Renderer) {
    if node.children.is_empty() {
        render_problem(
            template,
            fmt,
            "内容是空的：标题带要写一行字，或用 text=… 给",
        );
        return;
    }

    // 颜色只认十六进制：这个值会写进 style 属性，宽松了就等于允许塞任意声明
    let mut class = "banner".to_string();
    let mut style = String::new();
    if let Some(color) = template.param("color") {
        let Some(hex) = normalize_hex(color) else {
            render_problem(template, fmt, "color= 只认 #rgb 或 #rrggbb 这两种写法");
            return;
        };
        style.push_str(&format!("background: {hex}; color: {};", text_on(&hex)));
    }

    // 高度给的是**最小**高度：字大了往下长，不会被裁掉；给了它就顺带把内容在条里摆正
    if let Some(height) = template.param("height") {
        let Some(rule) = size_rule("min-height", height) else {
            render_problem(
                template,
                fmt,
                "height= 只认数字加单位（px / % / em / rem / vh / vw），例如 height=120px",
            );
            return;
        };
        style.push_str(&rule);
        class.push_str(" banner--sized");
    }

    // 圆角默认开着；要直角就写 rounded=off
    let rounded = template
        .param("rounded")
        .map(|value| value.trim().to_lowercase());
    match rounded.as_deref() {
        None | Some("" | "on" | "true" | "yes") => {}
        Some("off" | "false" | "no" | "0") => class.push_str(" banner--square"),
        Some(_) => {
            render_problem(template, fmt, "rounded= 只认 off（要直角）；不写就是圆角");
            return;
        }
    }

    let mut attrs: Vec<(&str, String)> = vec![("class", class)];
    if !style.is_empty() {
        attrs.push(("style", style));
    }

    fmt.cr();
    match inline_or_blocks(node) {
        // 常见情形：一行文字（解析出来就是一个段落）。横条里不能再套一个块，
        // 所以把段落的**行内内容**摊平放进来。
        InlineOrBlocks::Inline(inline) => {
            fmt.open("p", &attrs);
            fmt.contents(inline);
            fmt.close("p");
        }
        // 写成了好几段、或者塞了别的块：那不是一条横条了，照块的规矩渲染，
        // 内容不能丢，但换成 <div>，免得把一个块塞进 <p> 里。
        InlineOrBlocks::Blocks => {
            fmt.open("div", &attrs);
            fmt.cr();
            fmt.contents(&node.children);
            fmt.close("div");
        }
    }
    fmt.cr();
}

/// `::title text="大标题" color=#5b8dd6` —— **居中的大标题，没有背景**。
///
/// 与 `::banner` 是一对：那条是横条（有底色、占一整行），这个是标题（只有字）。
/// 卷首的题名、章节的分节标题用它；要一条有底色的横条就用 `::banner`。
/// `color=` 在这里给的是**字色**（没有底色可给）。
///
/// 内容与 `banner` 同一套规矩：块里写一行，或由 `text=` 给出，都按 markdown 渲染。
pub(super) fn render_title(template: &Template, node: &Node, fmt: &mut dyn Renderer) {
    if node.children.is_empty() {
        render_problem(template, fmt, "标题是空的：写一行字，或用 text=… 给");
        return;
    }

    let mut attrs: Vec<(&str, String)> = vec![("class", "title-block".to_string())];
    if let Some(color) = template.param("color") {
        let Some(hex) = normalize_hex(color) else {
            render_problem(template, fmt, "color= 只认 #rgb 或 #rrggbb 这两种写法");
            return;
        };
        attrs.push(("style", format!("color: {hex};")));
    }

    fmt.cr();
    match inline_or_blocks(node) {
        InlineOrBlocks::Inline(inline) => {
            fmt.open("p", &attrs);
            fmt.contents(inline);
            fmt.close("p");
        }
        // 写成了好几段：那就不是一个标题了，但内容不能丢，换成 <div> 照块渲染
        InlineOrBlocks::Blocks => {
            fmt.open("div", &attrs);
            fmt.cr();
            fmt.contents(&node.children);
            fmt.close("div");
        }
    }
    fmt.cr();
}

/// `::signature` —— 落款：右对齐的一段（署名、日期、"写在最后"的话）。
///
/// 内容照常按 markdown 渲染（落款里常有 `[[链接]]`、强调、甚至一张签名图）；
/// 硬换行保留（样式里那条 `white-space: pre-line`），
/// 所以"名字一行、日期一行"照写就是两行。
pub(super) fn render_signature(_template: &Template, node: &Node, fmt: &mut dyn Renderer) {
    fmt.cr();
    fmt.open("div", &[("class", "signature".to_string())]);
    fmt.cr();
    fmt.contents(&node.children);
    fmt.cr();
    fmt.close("div");
    fmt.cr();
}

/// "一行内容"的模板（横条、大标题）里的东西该按行内还是按块渲染
enum InlineOrBlocks<'a> {
    /// 一个段落：渲染它里面的行内内容
    Inline(&'a [Node]),
    /// 别的：按块渲染
    Blocks,
}

/// 只有一个段落时按行内渲染 —— 这是"一条横条 / 一个大标题"该有的样子：
/// 它们本身不是容器，里面再套一个块（`<p>` 里套 `<p>`）就散了
fn inline_or_blocks<'a>(node: &'a Node) -> InlineOrBlocks<'a> {
    match node.children.as_slice() {
        [only] if only.is::<Paragraph>() => InlineOrBlocks::Inline(&only.children),
        _ => InlineOrBlocks::Blocks,
    }
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
    fn signature_keeps_markdown_and_a_right_aligned_box() {
        let html = render("::signature\n  甲  \n  2026 年秋\n");
        assert!(html.contains(r#"<div class="signature">"#), "{html}");
        assert!(html.contains("甲"), "{html}");
        assert!(html.contains("2026 年秋"), "{html}");
    }
}
