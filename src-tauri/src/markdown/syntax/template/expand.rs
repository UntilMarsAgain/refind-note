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
            return Some(text_node(&format!(":: {name} —— {why}")));
        }
        None => return None,
    };

    // 转圈的保护：甲嵌乙、乙嵌甲会一直展开下去
    if crate::markdown::template_depth() >= MAX_TEMPLATE_DEPTH {
        return Some(text_node(&format!(
            ":: {name} —— 模板嵌套超过 {MAX_TEMPLATE_DEPTH} 层（是不是自己嵌自己？）"
        )));
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

/// 一段纯文字（用来把"读不出来""套得太深"这类话写在页面上）
fn text_node(text: &str) -> Node {
    let mut node = Node::new(Template {
        name: "problem".to_string(),
        params: Vec::new(),
        body: text.to_string(),
    });
    node.children
        .push(Node::new(markdown_it::parser::inline::Text {
            content: text.to_string(),
        }));
    node
}
