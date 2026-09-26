//! 模板标准库：随程序自带的那几个模板。
//!
//! 与指令表同样的规矩：**有什么模板只写在这一个地方**（下面那张表）。
//! 加一个模板 = 写一个渲染器 + 在表里加一行。
use super::dispatch::render_unknown;
use super::dispatch::TemplateRenderer;
use super::fill;
use super::parse::Template;
use markdown_it::{Node, Renderer};

/// 全部标准模板。
pub static TEMPLATES: &[(&str, TemplateRenderer)] = &[
    ("quote", render_quote),
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

/// `::css` —— 把内容（或 `src="页面名"` 指的模板页）当 CSS 注入页面。
///
/// 注入的 CSS 与界面在**同一个文档**里，所以本项目所有的 CSS 变量
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
    // 收进正文范围：不然一条 `* { }` 就能把整个界面改掉
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
