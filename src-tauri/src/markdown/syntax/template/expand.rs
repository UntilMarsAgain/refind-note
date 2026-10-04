//! **用户自定义模板**的展开：`::卡片` → 找 `Template:卡片` 那一页 → 填 `{{参数}}` → 解析成节点。
//!
//! 与标准模板（[`super::stdlib`]）的分界在名字：内置名字按标准渲染器走，非内置名字才来这里
//! 找同名页。所以这一步的产物**不是一个"渲染成某个样子"的块，而是一段内容** ——
//! 页面里怎么写就怎么长（见 [`ExpandedTemplate`]）。
//!
//! 两个"停下来说清楚"的情形，都返回一段说明文字而不是硬展开：
//!
//! - 这一页**在但读不出来**（没解锁、坏了）—— 报成"未知模板"会把作者引向错的方向；
//! - 套得太深（多半是模板自己嵌自己）—— [`MAX_TEMPLATE_DEPTH`] 是转圈的保护。

use super::fill;
use super::parse::Template;
use crate::markdown::syntax::wikilink;
use crate::vault::namespace::TEMPLATE_NAME;
use markdown_it::parser::block::BlockState;
use markdown_it::{Node, NodeValue, Renderer};

/// 用户模板最多套这么多层（再多就当是转圈了）
const MAX_TEMPLATE_DEPTH: usize = 12;

/// 展开一个**用户自定义模板**：`Template:名字` 那一页的正文 → 填 `{{参数}}` → 解析成节点。
///
/// 返回 `None` = "没有这一页"（调用方照旧报"未知模板"）；
/// 套得太深时返回一段说明文字 —— 那多半是模板自己嵌自己。
pub(super) fn expand_user_template(
    name: &str,
    params: Vec<(String, String)>,
    body: String,
    state: &mut BlockState,
) -> Option<Node> {
    let source = match crate::markdown::template_page(name) {
        Some(crate::markdown::TemplatePage::Ready(text)) => text,
        // 有这一页但读不出来（没解锁、坏了）：**说清原因**，别报成"未知模板"
        Some(crate::markdown::TemplatePage::Unreadable(why)) => {
            return Some(problem_node(name, &why));
        }
        None => return None,
    };

    // 转圈的保护：甲嵌乙、乙嵌甲会一直展开下去
    if crate::markdown::template_depth() >= MAX_TEMPLATE_DEPTH {
        return Some(problem_node(
            name,
            &format!("模板嵌套超过 {MAX_TEMPLATE_DEPTH} 层（是不是自己嵌自己？）"),
        ));
    }

    let template = Template {
        name: name.to_string(),
        params,
        body: body.clone(),
    };
    let filled = fill::substitute(&source, &template, &body);

    let md = state.md;
    let mut parsed = crate::markdown::deeper(|| md.parse(&filled));
    let mut node = Node::new(ExpandedTemplate);
    node.children = std::mem::take(&mut parsed.children);
    Some(node)
}

/// 用户模板展开后的节点：它自己没有 HTML，只把解析出来的子节点写出去
///
/// （与 `::html` 那种"渲染成某个样子"的模板不同 —— 用户模板就是**一段内容**，
/// 页面里怎么写就怎么长。）
#[derive(Debug)]
struct ExpandedTemplate;

impl NodeValue for ExpandedTemplate {
    fn render(&self, node: &Node, fmt: &mut dyn Renderer) {
        fmt.cr();
        fmt.contents(&node.children);
        fmt.cr();
    }
}

/// `problem` 节点：说清**出了什么事**，并且**给得出下一步**。
///
/// 只写一句"这一页需要口令"是把人堵在原地 —— 能做的下一步只有一个：去把那个模板页
/// 打开。而**打开的地方正是那个模板页本身**，所以这里必须挂一个指向它的链接：
/// 看到提示就能点过去改，而不是自己回想"刚才那个 `::卡片` 是哪一页"。
fn problem_node(name: &str, why: &str) -> Node {
    let page = format!("{TEMPLATE_NAME}:{name}");
    let mut node = Node::new(Template {
        name: "problem".to_string(),
        params: Vec::new(),
        body: why.to_string(),
    });
    node.children.push(wikilink::link_node(&page, &page));
    node
}

#[cfg(test)]
mod tests {
    use crate::markdown::{render_with_pages, TemplatePage, TemplatePages};
    use std::collections::HashMap;
    use std::sync::Arc;

    /// 模板页表里只有一页，且是**读不出来**的那种
    fn unreadable(name: &str) -> TemplatePages {
        let mut pages = HashMap::new();
        pages.insert(
            name.to_string(),
            TemplatePage::Unreadable("需要口令".into()),
        );
        Arc::new(pages)
    }

    #[test]
    fn an_unreadable_template_page_points_at_itself() {
        let html = render_with_pages("::卡片\n", None, Some(unreadable("卡片")));
        // 原因照旧要说清
        assert!(html.contains("需要口令"), "{html}");
        // **而且必须给得出下一步**：能做的下一步就是去把那一页打开
        assert!(html.contains(r#"class="wikilink""#), "{html}");
        assert!(html.contains(r#"data-doc="template:卡片""#), "{html}");
        // 不能是外链：内部链接不带 href，靠前端接住点击
        assert!(!html.contains(r#"href="#), "{html}");
    }

    #[test]
    fn nesting_over_the_limit_points_at_the_page_too() {
        // 套得太深也是同一个"改那一页就能解决"的问题，所以给一样的出口
        let html = render_with_pages("::甲\n", None, Some(unreadable("甲")));
        assert!(html.contains(r#"data-doc="template:甲""#), "{html}");
    }
}
