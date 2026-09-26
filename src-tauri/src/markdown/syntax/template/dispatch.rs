//! 按名字分发：先查标准模板表，再查**模板命名空间**里的同名页面；都没有就渲染一个报错框。
//!
//! 分发只认两处名字来源（表 + 模板页），别处不再各自认一遍。
use super::fill;
use super::parse::Template;
use super::stdlib::TEMPLATES;
use markdown_it::{Node, Renderer};

/// 一个模板渲染器：拿到头、拿到**解析好的内容节点**，往 `fmt` 里写 HTML。
///
/// 之所以把 `node` 一起给出去：内容已经由整个解析器解析成子节点，渲染器直接
/// `fmt.contents(&node.children)` 就能用 —— 引用块、提示框这类容器模板正需要它。
pub type TemplateRenderer = fn(&Template, &Node, &mut dyn Renderer);

thread_local! {
    /// 模板套模板的深度：`::a` 的模板页里又写 `::a` 会一直套下去，到上限就停手
    static DEPTH: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// 模板最多嵌套多少层。超了就当"查不到"，渲染成报错框 —— 报错看得见，比栈溢出好。
const MAX_DEPTH: usize = 8;

/// 分发：标准模板表 → 模板命名空间里的同名页面 → 报错框。
pub fn render(template: &Template, node: &Node, fmt: &mut dyn Renderer) {
    if let Some((_, renderer)) = TEMPLATES.iter().find(|(name, _)| *name == template.name) {
        renderer(template, node, fmt);
        return;
    }
    if render_from_namespace(template, fmt) {
        return;
    }
    render_unknown(template, fmt);
}

/// 模板命名空间里的同名页面。返回是否找到并渲染了。
fn render_from_namespace(template: &Template, fmt: &mut dyn Renderer) -> bool {
    let depth = DEPTH.with(std::cell::Cell::get);
    if depth >= MAX_DEPTH {
        return false;
    }
    let Some(source) = fill::source_of(template) else {
        return false;
    };
    // 模板页那份正文（`src` 能指向别的页面，所以这里仍然走 `source_of`）
    let Some(page) = crate::markdown::current_resolver()
        .and_then(|resolver| resolver.template(&template.name).map(str::to_string))
    else {
        return false;
    };
    let _ = source;

    DEPTH.with(|cell| cell.set(depth + 1));
    render_page(template, &page, fmt);
    DEPTH.with(|cell| cell.set(depth));

    true
}

/// 按页面的后缀决定怎么用它：`.html` / `.css` 走对应通道，其余当 markdown 模板。
fn render_page(template: &Template, page: &str, fmt: &mut dyn Renderer) {
    let filled = fill::substitute(page, template, &template.body);
    let lower = template.name.to_lowercase();

    if lower.ends_with(".html") {
        fmt.cr();
        if fill::allows_js(template) {
            fmt.text_raw(&filled);
        } else {
            fmt.text_raw(&fill::sanitize_html(&filled));
        }
        fmt.cr();
        return;
    }

    if lower.ends_with(".css") {
        fmt.cr();
        fmt.open("style", &[]);
        fmt.text_raw(&fill::scope_css(&fill::sanitize_css(&filled)));
        fmt.close("style");
        fmt.cr();
        return;
    }

    // markdown 模板：填好之后交给**整个解析器**再解析一遍，把结果原样放进来。
    // 于是模板页里可以写 markdown、内部链接，也可以再嵌别的模板。
    let resolver = crate::markdown::current_resolver();
    let html = crate::markdown::render_with(&filled, resolver.as_ref());
    fmt.cr();
    fmt.text_raw(&html);
    fmt.cr();
}

/// 名字查不到时的兜底：渲染一个框，把名字与参数原样列出，内容仍用代码块裹住。
///
/// 这是**给作者看的错误提示**，不是"不认识就静静丢掉"。参数回显成等号写法，
/// 所以从输出上就能看出解析成了什么 —— 引号去了哪儿、空格是否保住。
pub(super) fn render_unknown(template: &Template, fmt: &mut dyn Renderer) {
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
