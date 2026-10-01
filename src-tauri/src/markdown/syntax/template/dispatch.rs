//! 按名字分发：查标准模板表，查不到就渲染一个报错框。
use super::parse::Template;
use super::stdlib::TEMPLATES;
use markdown_it::{Node, Renderer};

/// 一个模板渲染器：拿到头、拿到**解析好的内容节点**，往 `fmt` 里写 HTML。
///
/// 之所以把 `node` 一起给出去：内容已经由整个解析器解析成子节点，渲染器直接
/// `fmt.contents(&node.children)` 就能用 —— 引用块、提示框这类容器模板正需要它。
pub type TemplateRenderer = fn(&Template, &Node, &mut dyn Renderer);

/// 分发：标准模板表 → 报错框。
pub fn render(template: &Template, node: &Node, fmt: &mut dyn Renderer) {
    if let Some((_, renderer)) = TEMPLATES.iter().find(|(name, _)| *name == template.name) {
        renderer(template, node, fmt);
        return;
    }
    render_unknown(template, fmt);
}

/// 名字查不到时的兜底：渲染一个框，把名字与参数原样列出，内容仍用代码块裹住。
///
/// 这是**给作者看的错误提示**，不是"不认识就静静丢掉"。参数回显成等号写法，
/// 所以从输出上就能看出解析成了什么 —— 引号去了哪儿、空格是否保住。
pub(super) fn render_unknown(template: &Template, fmt: &mut dyn Renderer) {
    fmt.cr();
    fmt.open(
        "div",
        &[("class", "template template--unknown".to_string())],
    );
    fmt.cr();
    head(fmt, "未知模板", template);
    fmt.cr();
    body_as_code(template, fmt);
    fmt.cr();
    fmt.close("div");
    fmt.cr();
}

/// 名字认识、但用法不对时的提示。
///
/// 与"未知模板"分开：一个是"没这个名字"，一个是"名字对、参数不对" ——
/// 提示词要跟着这个区别走，否则作者会往错的方向查。
pub(super) fn render_problem(template: &Template, fmt: &mut dyn Renderer, why: &str) {
    fmt.cr();
    fmt.open(
        "div",
        &[("class", "template template--problem".to_string())],
    );
    fmt.cr();
    head(fmt, "模板用法有问题", template);
    fmt.cr();
    fmt.open("p", &[("class", "template__why".to_string())]);
    fmt.text(why);
    fmt.close("p");
    fmt.cr();
    body_as_code(template, fmt);
    fmt.cr();
    fmt.close("div");
    fmt.cr();
}

/// 徽标 + 模板名 + 解析到的参数（回显成等号写法，一眼看出解析成了什么）
fn head(fmt: &mut dyn Renderer, badge: &str, template: &Template) {
    fmt.open("p", &[("class", "template__head".to_string())]);
    fmt.open("span", &[("class", "template__badge".to_string())]);
    fmt.text(badge);
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
}

/// 内容原样给出来（代码块）：写错了也不该让内容静静消失
fn body_as_code(template: &Template, fmt: &mut dyn Renderer) {
    fmt.open("pre", &[]);
    fmt.open("code", &[]);
    fmt.text(&template.body);
    fmt.close("code");
    fmt.close("pre");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 分节表里的名字都得是真模板：两张表一旦对不上，写下的名字会被当成"未知模板"，
    /// 而"要不要分节"却已经按这张表决定了 —— 这种半截状态最难查
    #[test]
    fn every_sectioned_name_is_a_real_template() {
        for name in super::super::stdlib::SECTIONED {
            assert!(
                TEMPLATES.iter().any(|(template, _)| template == name),
                "{name} 在分节表里，却不在模板表里"
            );
        }
    }

    #[test]
    fn dispatch_table_has_unique_names() {
        let mut names: Vec<&str> = TEMPLATES.iter().map(|(name, _)| *name).collect();
        let total = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(total, names.len(), "模板表里有重名");
    }
}
