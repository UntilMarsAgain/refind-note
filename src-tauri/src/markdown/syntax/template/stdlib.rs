//! 模板标准库：随程序自带的那几个模板。
//!
//! 与指令表同样的规矩：**有什么模板只写在这一个地方**（下面那张表）。
//! 加一个模板 = 写一个渲染器 + 在表里加一行。
use super::dispatch::{render_problem, render_unknown};
use super::dispatch::TemplateRenderer;
use super::fill;
use super::parse::Template;
use markdown_it::{Node, Renderer};

/// 全部标准模板。
pub static TEMPLATES: &[(&str, TemplateRenderer)] = &[
    ("quote", render_quote),
    ("code", render_code),
    ("aside", render_aside),
    ("fields", render_fields),
    ("banner", render_banner),
    ("image", render_image),
    ("css", render_css),
    ("html", render_html),
];

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

/// `::banner` —— 一条方框标题带（信息栏里那种居中的标题条）。
///
/// 文案取自块内容（一行），也可以用 `text=` 给。名字取 `banner` 而不是"方框标题"之类：
/// 它是一个**横条**，越短越不容易和别的模板混淆。
fn render_banner(template: &Template, _node: &Node, fmt: &mut dyn Renderer) {
    let text = match template.param("text") {
        Some(text) => text.to_string(),
        None => template.body.replace('\n', " ").trim().to_string(),
    };
    if text.is_empty() {
        render_problem(template, fmt, "内容是空的：标题带要写一行字，或用 text=… 给");
        return;
    }
    fmt.cr();
    fmt.open("p", &[("class", "banner".to_string())]);
    fmt.text(&text);
    fmt.close("p");
    fmt.cr();
}

/// `::image src=… align=left|center|right width=320 height=200 caption="说明"` —— 插入图片。
///
/// 尺寸是**上限**（`max-width` / `max-height`），图片不会被拉变形；数量与单位都受限
/// （见 [`size_rule`]），免得有人拿尺寸参数往 `style` 里塞别的东西。
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
    if let Some(rule) = template.param("width").and_then(|value| size_rule("max-width", value)) {
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
    if let Some(caption) = template.param("caption") {
        fmt.open("figcaption", &[]);
        fmt.text(caption);
        fmt.close("figcaption");
        fmt.cr();
    }
    fmt.close("figure");
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
    let Some(source) = fill::source_of(template) else {
        render_unknown(template, fmt);
        return;
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
    let Some(source) = fill::source_of(template) else {
        render_unknown(template, fmt);
        return;
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
