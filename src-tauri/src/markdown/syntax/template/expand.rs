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
        Some(crate::markdown::TemplatePage::Unreadable { reason, protection }) => {
            // 上锁 → 摆**解锁框**，不是一句提示。人和那个加密附件用的是同一个框。
            //
            // "要不要给口令输入框"从 `protection.symmetric` 来 —— 后端此刻已经知道，
            // 不必让前端再猜一次（猜错是静默的，见 `TemplatePage::Unreadable`）。
            return Some(decrypt_node(name, &reason, protection.symmetric));
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

/// 造一个"出错了"的内部节点：说清出了什么事，并且给得出下一步。
///
/// 两类节点共用它，区别只在 `name` 与参数：
///
/// - [`problem_node`]：说法本身有问题（模板套模板超层数）。只给一句说明。
/// - [`decrypt_node`]：东西没问题，只是它上了锁。给**解锁框**，能在原地解锁。
///
/// 两者都挂一个指向那个模板页的链接：口令打不开时，能做的下一步仍然是去把
/// 那一页改掉 —— 看到提示就能点过去，而不是自己回想"刚才那个 `::卡片` 是哪一页"。
fn locked_node(name: &str, why: &str, kind: &str, needs_passphrase: bool) -> Node {
    let page = format!("{TEMPLATE_NAME}:{name}");
    let mut node = Node::new(Template {
        name: kind.to_string(),
        // `decrypt` 要目标与名字；**不要**在这里猜"要不要口令" ——
        // 那是 `commands::lock::lock_state` 的活（它真的去问那一版的头），
        // 这里猜的话模板页与文件就会有两套判断。
        params: vec![
            ("target".to_string(), page.clone()),
            ("label".to_string(), name.to_string()),
            (
                "passphrase".to_string(),
                if needs_passphrase { "yes" } else { "no" }.to_string(),
            ),
        ],
        body: why.to_string(),
    });
    node.children.push(wikilink::link_node(&page, &page));
    node
}

/// 说法本身有问题 —— 不给解锁框
fn problem_node(name: &str, why: &str) -> Node {
    locked_node(name, why, "problem", false)
}

/// 上了锁 —— 给解锁框，参数见 [`dispatch::render_decrypt`]
///
/// `needs_passphrase` 由**后端**给（它刚读过那一页的封装头）：对称层要问口令，
/// gpg 层不问 —— 它问的是钥匙串或智能卡。
fn decrypt_node(name: &str, why: &str, needs_passphrase: bool) -> Node {
    locked_node(name, why, "decrypt", needs_passphrase)
}

#[cfg(test)]
mod tests {
    use crate::markdown::{render_with_pages, TemplatePage, TemplatePages};
    use std::collections::HashMap;
    use std::sync::Arc;

    /// 模板页表里只有一页，且是**读不出来**的那种
    fn unreadable(name: &str) -> TemplatePages {
        unreadable_with(name, "需要口令", false)
    }

    /// 读不出来，且**说清是哪一层**（决定解锁框要不要给口令输入框）
    fn unreadable_with(name: &str, reason: &str, symmetric: bool) -> TemplatePages {
        let mut pages = HashMap::new();
        pages.insert(
            name.to_string(),
            TemplatePage::Unreadable {
                reason: reason.to_string(),
                protection: crate::storage::codec::Protection {
                    symmetric,
                    ..crate::storage::codec::Protection::plain()
                },
            },
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

    /// 上了锁 → 摆的是**解锁框**（`data-decrypt`），不是一句提示。
    ///
    /// 这一条钉的是"人在原地就能解锁"这件事：只给一句"需要口令"的话，
    /// 能做的下一步只剩"去把那一页打开"，那还得改模板；框是可以直接输口令的。
    #[test]
    fn a_locked_template_page_gets_an_unlock_box_not_just_a_hint() {
        let html = render_with_pages("::卡片\n", None, Some(unreadable("卡片")));

        // 框本身：哪一种、哪一份、为什么
        assert!(html.contains(r#"data-decrypt="yes""#), "{html}");
        assert!(html.contains(r#"data-decrypt-kind="page""#), "{html}");
        // 命名空间是小写的（`template:卡片`）—— 与 `wikilink` 写出来的那个一致
        assert!(
            html.contains(r##"data-decrypt-title="template:卡片""##),
            "{html}"
        );
        assert!(html.contains(r#"data-decrypt-reason="需要口令""#), "{html}");
        // 旧的那套还在（说清原因 + 指路），但**不再**是唯一出口
        assert!(html.contains(r#"data-doc="template:卡片""#), "{html}");
    }

    /// 解锁框只有标记，**没有**输入框与按钮。
    ///
    /// 那两样由前端 `dom/decrypt.ts` 补 —— 因为同样的框还有另一个来路（加密附件），
    /// 两边要共用同一个构造器，框的样子只能在前端定义一次。后端只负责说
    /// "这里有个解锁框，它是这样"。
    #[test]
    fn the_unlock_box_is_only_a_marker_so_both_callers_share_one_builder() {
        let html = render_with_pages("::卡片\n", None, Some(unreadable("卡片")));
        // 前端造的那两样，后端不该自己造 —— 否则两条来路会长成两个样子
        assert!(!html.contains(r#"type="password""#), "{html}");
        assert!(!html.contains(r#"class="file-locked__go""#), "{html}");
        assert!(!html.contains(r#"class="file-locked__input""#), "{html}");
    }

    /// 解锁框上**要不要口令输入框**由后端写进标记里，前端不必猜。
    ///
    /// 这一条钉的是"只有后端知道"：它在渲染的时候刚读过那一页的封装头
    /// （`TemplatePage::Unreadable` 带着 `protection` 就是为此）。
    /// 猜错是**静默**的 —— gpg 的框上多一个输入框（人白输一次），
    /// 或者口令的框上没有（人卡在那儿，什么也做不了）。
    #[test]
    fn the_marker_carries_the_real_answer_about_whether_a_passphrase_is_wanted() {
        let symmetric = render_with_pages(
            "::卡片\n",
            None,
            Some(unreadable_with("卡片", "需要口令", true)),
        );
        assert!(
            symmetric.contains(r#"data-decrypt-needs-passphrase="yes""#),
            "对称层要问口令：{symmetric}"
        );

        let gpg = render_with_pages(
            "::卡片\n",
            None,
            Some(unreadable_with("卡片", "gpg 那边出错了：没有私钥", false)),
        );
        assert!(
            gpg.contains(r#"data-decrypt-needs-passphrase="no""#),
            "gpg 层不问口令（它问的是钥匙串）：{gpg}"
        );
        // 原因照原样传下去：gpg 的失败有好几种，不该被压成同一句话
        assert!(gpg.contains("没有私钥"), "{gpg}");
    }

    /// 说法本身有问题（套太深）**不给**解锁框。
    ///
    /// 那种情况输了口令问题还在 —— 给个输入框是荒唐的。这一条钉住两个模板的分工。
    #[test]
    fn a_usage_problem_is_not_given_an_unlock_box() {
        // 甲嵌乙、乙嵌甲会一直展开下去，于是超过层数上限
        let mut pages = HashMap::new();
        pages.insert("甲".to_string(), TemplatePage::Ready("::乙\n".into()));
        pages.insert("乙".to_string(), TemplatePage::Ready("::甲\n".into()));
        let html = render_with_pages("::甲\n", None, Some(Arc::new(pages)));

        assert!(html.contains("模板嵌套超过"), "{html}");
        assert!(
            !html.contains(r#"data-decrypt="yes""#),
            "套太深不是上锁，不该给解锁框：{html}"
        );
    }

    #[test]
    fn nesting_over_the_limit_points_at_the_page_too() {
        // 套得太深也是同一个"改那一页就能解决"的问题，所以给一样的出口
        let html = render_with_pages("::甲\n", None, Some(unreadable("甲")));
        assert!(html.contains(r#"data-doc="template:甲""#), "{html}");
    }
}
