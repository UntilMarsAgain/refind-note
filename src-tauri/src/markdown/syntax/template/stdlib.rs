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
//! 与指令表同样的规矩：**有什么模板只写在这一个地方**（下面那张表）。
//! 加一个模板 = 写一个渲染器 + 在表里加一行。
use super::dispatch::render_problem;
use super::dispatch::TemplateRenderer;
use super::fill;
use super::parse::Template;
use super::Section;
use markdown_it::plugins::cmark::block::paragraph::Paragraph;
use markdown_it::{Node, Renderer};

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
];

/// 内容按 `[标签]` 分节的模板。
///
/// 分节发生在**扫描时**（各节要各自解析成 markdown，见 `mod.rs`），所以扫描器得知道
/// 哪些模板分节 —— 这张表因此和 `TEMPLATES` 放在一起：两张表加起来才是"有哪些模板"。
pub static SECTIONED: &[&str] = &["tabs", "theme"];

/// 这个名字是内置模板吗（不是的话，扫描器会去 `Template:` 命名空间找同名的页）
pub fn is_builtin(name: &str) -> bool {
    TEMPLATES.iter().any(|(builtin, _)| *builtin == name)
}

/// 这个模板是不是分节的（扫描器用）
pub fn takes_sections(name: &str) -> bool {
    SECTIONED.contains(&name)
}

/// 取分节模板的几节：`(标签原样, 那一节的节点)`。
///
/// 标签**不做大小写转换**：它是显示用的字（`[API]` 该照原样出现在选项卡上），
/// 要按大小写不敏感地认标签的地方（`::theme`）自己转。
fn sections(node: &Node) -> Vec<(&str, &Node)> {
    node.children
        .iter()
        .filter_map(|child| {
            child
                .cast::<Section>()
                .map(|section| (section.label.as_str(), child))
        })
        .collect()
}

/// 署名前面那条横线。
///
/// 单独拎出来是因为它只该出现在一个地方：想换成两个破折号、或者换成普通连字符，
/// 改这里一处即可。
const ORIGIN_DASH: &str = "—";

/// `::quote origin="署名"` —— 效果等同 markdown 的 `>`，但可以在参数里给一个署名，
/// 渲染到右下角。
///
/// 内容由整个解析器解析过（见 `mod.rs`），所以里面可以写 markdown，也可以再嵌 `::quote`。
fn render_quote(template: &Template, node: &Node, fmt: &mut dyn Renderer) {
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
fn render_aside(template: &Template, node: &Node, fmt: &mut dyn Renderer) {
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
/// 值用的是**原文**（不解析 markdown）：这一栏是"查参数"用的，一行一项最清楚。
/// 没有 `|` 的行把整行当标签、值留空 —— 宁可少一栏，也不丢作者写下的字。
fn render_fields(template: &Template, _node: &Node, fmt: &mut dyn Renderer) {
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
fn render_banner(template: &Template, node: &Node, fmt: &mut dyn Renderer) {
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

/// `#abc` / `#aabbcc` → 规范的 `#aabbcc`；别的写法一律不认
fn normalize_hex(text: &str) -> Option<String> {
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
fn text_on(background: &str) -> &'static str {
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

/// `::image src=…` —— 插入图片。`src` 必给；尺寸、位置、注释都可选：
///
/// - `align=left|center|right`（默认居中）；
/// - `width=` / `height=` —— 都是**上限**（`max-width` / `max-height`），图片不会被拉变形，
///   数量与单位都受限（见 [`size_rule`]），免得有人拿尺寸参数往 `style` 里塞别的东西；
/// - 注释有两种写法：**图片下面写一行**（最自然），或者 `caption="…"`。
///
/// 协议只挡能执行或能外传数据的三种：`javascript:` / `data:` / `vbscript:`。
fn render_image(template: &Template, _node: &Node, fmt: &mut dyn Renderer) {
    let Some(source) = template.param("src") else {
        render_problem(template, fmt, "缺少 src=…：图片模板至少要给出图片地址");
        return;
    };
    let lowered = source.trim().to_lowercase();
    if ["javascript:", "data:", "vbscript:"]
        .iter()
        .any(|bad| lowered.starts_with(bad))
    {
        render_problem(
            template,
            fmt,
            "src 用的是不允许的协议（javascript / data / vbscript 会被拒绝）",
        );
        return;
    }

    let align = match template.param("align").map(str::trim) {
        Some("left") => "left",
        Some("right") => "right",
        _ => "center",
    };

    let mut style = String::new();
    if let Some(rule) = template
        .param("width")
        .and_then(|value| size_rule("max-width", value))
    {
        style.push_str(&rule);
    }
    if let Some(rule) = template
        .param("height")
        .and_then(|value| size_rule("max-height", value))
    {
        style.push_str(&rule);
    }

    fmt.cr();
    fmt.open("figure", &[("class", format!("image image--{align}"))]);
    fmt.cr();
    let mut attrs: Vec<(&str, String)> = vec![
        ("src", source.trim().to_string()),
        (
            "alt",
            template.param("alt").unwrap_or("").trim().to_string(),
        ),
        // 长文里的图片不该抢首屏带宽
        ("loading", "lazy".to_string()),
    ];
    if !style.is_empty() {
        attrs.push(("style", style));
    }
    fmt.self_close("img", &attrs);
    fmt.cr();
    // 注释：块里的那一行（最自然的写法）优先用 `caption=` 覆盖。
    // 块内容按行取、去掉缩进再拼成一句：它在块里是被缩进的，不该带着空格显示。
    let caption = match template.param("caption") {
        Some(caption) => caption.trim().to_string(),
        None => template
            .body
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join(" "),
    };
    if !caption.is_empty() {
        fmt.open("figcaption", &[]);
        fmt.text(&caption);
        fmt.close("figcaption");
        fmt.cr();
    }
    fmt.close("figure");
    fmt.cr();
}

/// `::video src=片子.mp4` / `::audio src=录音.mp3` —— 摆一个播放器。
///
/// 与 `::image` 同一套参数：`src` 必给，`width=` / `height=` 是**上限**，
/// 注释写在块里一行或由 `caption=` 给出。`align=` 只对视频有意义（音频是个窄条）。
///
/// 名字里的 `src` 写的是**仓库里的名字**（`片子.mp4`），与 `![]()` 里写的是同一个东西；
/// 换成能取到字节的地址是前端注入之后的事。
fn render_video(template: &Template, node: &Node, fmt: &mut dyn Renderer) {
    render_media(template, node, fmt, "video");
}

/// 见 [`render_video`]
fn render_audio(template: &Template, node: &Node, fmt: &mut dyn Renderer) {
    render_media(template, node, fmt, "audio");
}

/// 视频与音频只差一个标签名：源、尺寸、注释、位置的规矩完全一样
fn render_media(template: &Template, _node: &Node, fmt: &mut dyn Renderer, kind: &str) {
    let Some(source) = template.param("src") else {
        render_problem(template, fmt, "缺少 src=…：至少要给出文件名字");
        return;
    };
    let lowered = source.trim().to_lowercase();
    if ["javascript:", "data:", "vbscript:"]
        .iter()
        .any(|bad| lowered.starts_with(bad))
    {
        render_problem(
            template,
            fmt,
            "src 用的是不允许的协议（javascript / data / vbscript 会被拒绝）",
        );
        return;
    }

    let align = match template.param("align").map(str::trim) {
        Some("left") => "left",
        Some("right") => "right",
        _ => "center",
    };

    let mut style = String::new();
    if let Some(rule) = template
        .param("width")
        .and_then(|value| size_rule("max-width", value))
    {
        style.push_str(&rule);
    }
    if let Some(rule) = template
        .param("height")
        .and_then(|value| size_rule("max-height", value))
    {
        style.push_str(&rule);
    }

    fmt.cr();
    fmt.open(
        "figure",
        &[("class", format!("media media--{kind} media--{align}"))],
    );
    fmt.cr();
    let mut attrs: Vec<(&str, String)> = vec![
        ("src", source.trim().to_string()),
        // 播放器该有的控件一个不少；预加载只取头，别让长片子一打开就拖满带宽
        ("controls", "controls".to_string()),
        ("preload", "metadata".to_string()),
    ];
    if !style.is_empty() {
        attrs.push(("style", style));
    }
    fmt.self_close(kind, &attrs);
    fmt.cr();
    if let Some(caption) = caption_of(template) {
        fmt.open("figcaption", &[]);
        fmt.text(&caption);
        fmt.close("figcaption");
        fmt.cr();
    }
    fmt.close("figure");
    fmt.cr();
}

/// 注释：`caption=` 优先，其次块里写着的那一行（缩进与空行都不算）
fn caption_of(template: &Template) -> Option<String> {
    match template.param("caption") {
        Some(caption) => Some(caption.trim().to_string()).filter(|text| !text.is_empty()),
        None => {
            let text = template
                .body
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            Some(text).filter(|text| !text.is_empty())
        }
    }
}

/// `::title text="大标题" color=#5b8dd6` —— **居中的大标题，没有背景**。
///
/// 与 `::banner` 是一对：那条是横条（有底色、占一整行），这个是标题（只有字）。
/// 卷首的题名、章节的分节标题用它；要一条有底色的横条就用 `::banner`。
/// `color=` 在这里给的是**字色**（没有底色可给）。
///
/// 内容与 `banner` 同一套规矩：块里写一行，或由 `text=` 给出，都按 markdown 渲染。
fn render_title(template: &Template, node: &Node, fmt: &mut dyn Renderer) {
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

/// `::tabs` —— 选项卡：块里用 `[标签]` 分节，一节就是一个面板。
///
/// ```text
/// ::tabs
///   [北岸]
///   北岸的走法……
///   [南岸]
///   南岸的走法……
/// ```
///
/// 后端只产出**结构**（一排标签 + 各面板），切换由前端接上 —— 渲染器往输出里塞不了
/// `onclick`，塞了也过不了 CSP，而且阅读页与编辑器预览是两处注入，行为该由前端一处管。
/// 第一节默认选中。标签是显示用的字，原样保留（`[API]` 不会变成 `[api]`）。
///
/// `[标签]` 与本节的其他行**左对齐**：写得更深就当成里层内容了（嵌套的选项卡靠这条分得开）。
fn render_tabs(template: &Template, node: &Node, fmt: &mut dyn Renderer) {
    let sections = sections(node);
    if sections.is_empty() {
        render_problem(template, fmt, "选项卡是空的：每一节要以 [标签] 开头");
        return;
    }
    if sections.iter().any(|(label, _)| label.is_empty()) {
        render_problem(
            template,
            fmt,
            "有内容写在第一个 [标签] 之前：选项卡的每一节都要以 [标签] 开头",
        );
        return;
    }

    fmt.cr();
    fmt.open("div", &[("class", "tabs".to_string())]);
    fmt.cr();
    fmt.open(
        "div",
        &[
            ("class", "tabs__bar".to_string()),
            ("role", "tablist".to_string()),
        ],
    );
    for (index, (label, _)) in sections.iter().enumerate() {
        let selected = index == 0;
        // 用真按钮：Tab 键走得到、回车就切（切换的接线在前端）
        let attrs: Vec<(&str, String)> = vec![
            (
                "class",
                if selected {
                    "tabs__tab tabs__tab--on".to_string()
                } else {
                    "tabs__tab".to_string()
                },
            ),
            ("type", "button".to_string()),
            ("role", "tab".to_string()),
            ("data-tab", index.to_string()),
            (
                "aria-selected",
                if selected { "true" } else { "false" }.to_string(),
            ),
        ];
        fmt.open("button", &attrs);
        fmt.text(label);
        fmt.close("button");
    }
    fmt.cr();
    fmt.close("div");
    fmt.cr();

    for (index, (_, content)) in sections.iter().enumerate() {
        let mut attrs: Vec<(&str, String)> = vec![
            ("class", "tabs__panel".to_string()),
            ("role", "tabpanel".to_string()),
            ("data-panel", index.to_string()),
        ];
        if index != 0 {
            attrs.push(("hidden", "hidden".to_string()));
        }
        fmt.open("div", &attrs);
        fmt.cr();
        fmt.contents(&content.children);
        fmt.cr();
        fmt.close("div");
    }
    fmt.cr();
    fmt.close("div");
    fmt.cr();
}

/// `::theme` —— 同一段内容，深浅色各给一版。
///
/// ```text
/// ::theme
///   [light]
///   浅色下显示这一段（浅底上的图，用的是浅色描边）
///   [dark]
///   深色下显示这一段
/// ```
///
/// **两节都会渲染进 HTML**，显示哪一节由样式按当前主题决定（`<html data-theme="…">`
/// 是主题的落点，见前端 `core/theme.ts`）。这样切主题不必重新渲染正文 ——
/// 就像别的样式一样，当场就变。
///
/// 写在第一个标签**之前**的内容两边都显示（`[both]` 同义）：前言、说明常常要这样写。
/// 与 `::tabs` 同一个规矩：`[标签]` 要与本节的其他行**左对齐**。
fn render_theme(template: &Template, node: &Node, fmt: &mut dyn Renderer) {
    let sections = sections(node);
    if sections.is_empty() {
        render_problem(template, fmt, "要分节：每一节以 [light] 或 [dark] 开头");
        return;
    }

    // 先整块认一遍再渲染：标签写错了就整块报出来，免得渲染半截
    let mut parts: Vec<(&str, &Node)> = Vec::new();
    for (label, content) in &sections {
        let slot = match label.to_lowercase().as_str() {
            "" | "both" | "all" => "",
            "light" => "light",
            "dark" => "dark",
            _ => {
                let why = format!(
                    "认不出这一节的标签 [{label}]：只认 [light] 与 [dark]  \
                     （写在最前面的内容两边都显示）"
                );
                render_problem(template, fmt, &why);
                return;
            }
        };
        parts.push((slot, content));
    }

    fmt.cr();
    fmt.open("div", &[("class", "theme".to_string())]);
    for (slot, content) in parts {
        let class = match slot {
            "light" => "theme__part theme__part--light",
            "dark" => "theme__part theme__part--dark",
            _ => "theme__part",
        };
        fmt.cr();
        fmt.open("div", &[("class", class.to_string())]);
        fmt.cr();
        fmt.contents(&content.children);
        fmt.cr();
        fmt.close("div");
    }
    fmt.cr();
    fmt.close("div");
    fmt.cr();
}

/// `::js` —— 把内容当**原始 JavaScript** 注入这一页，随页面加载执行。
///
/// 与 `::html` 一个规矩：内容原样，`{{参数}}` 会替换（见 [`fill`]）。不过滤 ——
/// 打开它（写下 `::js` 本身）就是一次显式的决定，与 `::html js` 是同一个意思。
///
/// 注入的脚本里若出现 `</script`，HTML 解析器会当场收尾，后面的字会漏到页面上，
/// 所以那一处先转义（见 [`fill::escape_script_end`]）。
///
/// 前端在正文注入 DOM 之后把 `<script>` 重新装成真节点（`v-html` 塞进去的脚本不会执行），
/// 所以这段代码才会真的跑起来。
fn render_js(template: &Template, _node: &Node, fmt: &mut dyn Renderer) {
    let source = match fill::source_of(template) {
        Ok(source) => source,
        Err(why) => {
            render_problem(template, fmt, &why);
            return;
        }
    };
    let filled = fill::substitute(&source, template, &template.body);

    fmt.cr();
    fmt.open("script", &[("class", "note-js".to_string())]);
    fmt.text_raw(&fill::escape_script_end(&filled));
    fmt.close("script");
    fmt.cr();
}

/// 尺寸参数 → 一条 CSS 声明。只认"数字 + 可选白名单单位"，别的写法一律不认。
///
/// 严格是有理由的：这个值会被写进 `style` 属性，宽松了就等于允许往里面塞任意声明
/// （比如 `width="1px; background: url(//追踪地址)"`）。
fn size_rule(name: &str, value: &str) -> Option<String> {
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

/// `::mermaid` —— 画一张图（[mermaid](https://mermaid.js.org) 的语法）。
///
/// 这里只把**原文**放进 `<pre class="mermaid">`，画图在前端（mermaid 是 JS）。
/// 与 `::code` 同一个道理：图定义里的 `-->`、`[]`、`{}` 都是它自己的记号，
/// 绝不能按 markdown 解析。
///
/// 原文同时留一份在 `data-source` 上：mermaid 画完会把元素内容换成 SVG，
/// 切主题要重画时就靠这份原文还原（见前端 `dom/diagrams.ts`）。
fn render_mermaid(template: &Template, _node: &Node, fmt: &mut dyn Renderer) {
    let source = template.body.trim();
    if source.is_empty() {
        render_problem(template, fmt, "里面还没有图 —— 写一段 mermaid 定义");
        return;
    }

    fmt.cr();
    fmt.open("div", &[("class", "diagram".to_string())]);
    fmt.cr();
    fmt.open(
        "pre",
        &[
            ("class", "mermaid".to_string()),
            ("data-source", source.to_string()),
        ],
    );
    fmt.text(source);
    fmt.close("pre");
    fmt.cr();
    fmt.close("div");
    fmt.cr();
}

/// `::math` —— 独立成段（或多行）的公式，TeX 原文，前端用 KaTeX 排版。
///
/// 正文**不按 markdown 解析**：公式里的 `*`、`_`、`\` 都是数学的一部分
/// （与 `::code`、`::mermaid` 同一条规矩）。
/// 只写一行的公式也可以用 `$…$` / `$$…$$`，那是 `syntax/math.rs` 认的。
fn render_math(template: &Template, _node: &Node, fmt: &mut dyn Renderer) {
    let tex = template.body.trim();
    if tex.is_empty() {
        render_problem(template, fmt, "里面还没有公式 —— 写一段 TeX");
        return;
    }

    fmt.cr();
    fmt.open(
        "div",
        &[
            ("class", "math math--display".to_string()),
            ("data-tex", tex.to_string()),
        ],
    );
    // 排版之前看到的是原文，取不到 KaTeX 也不至于是一片空白
    fmt.text(tex);
    fmt.close("div");
    fmt.cr();
}

/// `::signature` —— 落款：右对齐的一段（署名、日期、"写在最后"的话）。
///
/// 内容照常按 markdown 渲染（落款里常有 `[[链接]]`、强调、甚至一张签名图）；
/// 硬换行保留（样式里那条 `white-space: pre-line`），
/// 所以"名字一行、日期一行"照写就是两行。
fn render_signature(_template: &Template, node: &Node, fmt: &mut dyn Renderer) {
    fmt.cr();
    fmt.open("div", &[("class", "signature".to_string())]);
    fmt.cr();
    fmt.contents(&node.children);
    fmt.cr();
    fmt.close("div");
    fmt.cr();
}

/// `::code lang=rust lines=off start=10 highlight=2-3` —— 像 markdown 的代码块，
/// 但能控制行号与要强调的行。
///
/// 这里**只输出代码原文与几个 `data-*` 提示**：行号列与强调色带由前端落地。
/// 为什么不在这里生成行号或拆行：拆行会破坏 highlight.js 的分词（它的 span 可能跨行），
/// 而"界面怎么显示"本来就属于前端。
fn render_code(template: &Template, _node: &Node, fmt: &mut dyn Renderer) {
    let language = template.param("lang").unwrap_or("").trim();

    let mut attrs: Vec<(&str, String)> = vec![("class", "template-code".to_string())];
    if let Some(lines) = template.param("lines") {
        let off = matches!(lines.trim(), "off" | "false" | "no");
        attrs.push(("data-lines", if off { "off" } else { "on" }.to_string()));
    }
    if let Some(start) = template.param("start") {
        attrs.push(("data-line-start", start.trim().to_string()));
    }
    if let Some(highlight) = template.param("highlight") {
        attrs.push(("data-highlight", highlight.trim().to_string()));
    }

    let code_attrs: Vec<(&str, String)> = if language.is_empty() {
        Vec::new()
    } else {
        vec![("class", format!("language-{language}"))]
    };

    fmt.cr();
    fmt.open("pre", &attrs);
    fmt.open("code", &code_attrs);
    // 原文照收：代码里的 markdown 记号**不该**被解析
    fmt.text(&template.body);
    fmt.close("code");
    fmt.close("pre");
    fmt.cr();
}

/// `::css` —— 把内容（或 `src="页面名"` 指的模板页）当 CSS 注入页面。
///
/// 注入的 CSS 与界面在**同一个文档**里（作用域收在这一页的内容上），所以本项目所有的 CSS 变量
/// （`--accent`、`--accent-solid`、`--link-blue`、`--text-dim`、`--surface`…）
/// 在这里用 `var()` 直接就能取到 —— 这正是"自定义模板能跟着主题走"的关键。
///
/// 内容里的 `{{参数}}` 会被替换；`</style` 会被去掉，免得提前闭合样式块。
fn render_css(template: &Template, _node: &Node, fmt: &mut dyn Renderer) {
    let source = match fill::source_of(template) {
        Ok(source) => source,
        Err(why) => {
            render_problem(template, fmt, &why);
            return;
        }
    };
    let css = fill::sanitize_css(&fill::substitute(&source, template, &template.body));
    // 收进这一页：不然一条 `* { }` 就能把整个界面改掉
    let css = fill::scope_css(&css);
    fmt.cr();
    fmt.open("style", &[]);
    fmt.text_raw(&css);
    fmt.close("style");
    fmt.cr();
}

/// `::html` —— 把内容当**原始 HTML** 注入。
///
/// 默认**过滤**掉会执行脚本的东西（`<script>`、`on*=` 事件属性、`javascript:` 协议）；
/// 只有显式写了 `js`（或 `js=true`）才原样放行。默认安全，要开就得自己写出来。
fn render_html(template: &Template, _node: &Node, fmt: &mut dyn Renderer) {
    let source = match fill::source_of(template) {
        Ok(source) => source,
        Err(why) => {
            render_problem(template, fmt, &why);
            return;
        }
    };
    let filled = fill::substitute(&source, template, &template.body);
    let html = if fill::allows_js(template) {
        filled
    } else {
        fill::sanitize_html(&filled)
    };
    fmt.cr();
    fmt.text_raw(&html);
    fmt.cr();
}
