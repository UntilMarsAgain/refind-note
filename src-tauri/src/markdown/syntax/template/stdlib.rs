//! 模板标准库：随程序自带的那几个模板。
//!
//! 与指令表同样的规矩：**有什么模板只写在这一个地方**（下面那张表）。
//! 加一个模板 = 写一个渲染器 + 在表里加一行。
use super::dispatch::TemplateRenderer;
use super::parse::Template;
use markdown_it::{Node, Renderer};

/// 全部标准模板。
pub static TEMPLATES: &[(&str, TemplateRenderer)] = &[("quote", render_quote)];

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
        fmt.text(origin);
        fmt.close("p");
    }
    fmt.cr();
    fmt.close("blockquote");
    fmt.cr();
}
