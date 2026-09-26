//! 按名字分发到渲染器；查不到就渲染一个"未知模板"的框。
//!
//! 分发**只认 [`super::stdlib::TEMPLATES`] 那一张表**：名字写在哪里，就只写在那里。
use super::parse::Template;
use super::stdlib::TEMPLATES;
use markdown_it::{Node, Renderer};

/// 一个模板渲染器：拿到头、拿到**解析好的内容节点**，往 `fmt` 里写 HTML。
///
/// 之所以把 `node` 一起给出去：内容已经由整个解析器解析成子节点，渲染器直接
/// `fmt.contents(&node.children)` 就能用 —— 引用块、提示框这类容器模板正需要它。
pub type TemplateRenderer = fn(&Template, &Node, &mut dyn Renderer);

/// 分发：查表，查不到交给"未知模板"。
pub fn render(template: &Template, node: &Node, fmt: &mut dyn Renderer) {
    match TEMPLATES.iter().find(|(name, _)| *name == template.name) {
        Some((_, renderer)) => renderer(template, node, fmt),
        None => render_unknown(template, fmt),
    }
}

/// 名字查不到时的兜底：渲染一个框，把名字与参数原样列出，内容仍用代码块裹住。
///
/// 这是**给作者看的错误提示**，不是"不认识就静静丢掉"。参数回显成等号写法，
/// 所以从输出上就能看出解析成了什么 —— 引号去了哪儿、空格是否保住。
fn render_unknown(template: &Template, fmt: &mut dyn Renderer) {
    fmt.cr();
    fmt.open("div", &[("class", "template template--unknown".to_string())]);
    fmt.cr();
    fmt.open("p", &[("class", "template__head".to_string())]);
    fmt.open("span", &[("class", "template__badge".to_string())]);
    fmt.text("未知模板");
    fmt.close("span");
    fmt.open("code", &[("class", "template__name".to_string())]);
    fmt.text(&template.name);
    fmt.close("code");
    for (key, value) in &template.params {
        fmt.open("code", &[("class", "template__param".to_string())]);
        let shown = if value.is_empty() {
            key.clone()
        } else {
            format!("{key}={value}")
        };
        fmt.text(&shown);
        fmt.close("code");
    }
    fmt.close("p");
    fmt.cr();
    // 内容原样给出来：写错了也不该让内容静静消失
    fmt.open("pre", &[]);
    fmt.open("code", &[]);
    fmt.text(&template.body);
    fmt.close("code");
    fmt.close("pre");
    fmt.cr();
    fmt.close("div");
    fmt.cr();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatch_table_has_unique_names() {
        let mut names: Vec<&str> = TEMPLATES.iter().map(|(name, _)| *name).collect();
        let total = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(total, names.len(), "模板表里有重名");
    }
}
