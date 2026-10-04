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
            // 读不出来 → **当作作者写了一个 `::decrypt page=那一页`**（作者定的）。
            //
            // 所以这里走**同一个** `dispatch::render_decrypt`，而不是另做一套渲染：
            // 两条来路（作者亲手写的、解析器自己摆的）框的样子与行为只有一处定义，
            // 改一边另一边不会安静地变样。
            //
            // 此刻已经知道的两样**在这里白拿**：渲染器会自己从同一张表再问一遍，
            // 答案一致，而少一份"这里算一遍、那里算一遍"就不会算歪。
            let _ = (reason, protection);
            return Some(decrypt_node(name));
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

/// 造一个 `::decrypt` 节点：**当作**作者写了 `::decrypt page=那一页`。
///
/// 与作者亲手写的走同一个渲染器（见上面那处调用），所以这里只补两样作者不会写的东西：
///
/// - `internal`：让 `render_decrypt_box` 在框下面挂"去把那一页改掉"的链接 ——
///   作者写的那种 `children` 是**底部内容**，锁着的时候不该显示出来；
/// - children 上那个指向模板页的链接（口令打不开时，人还能去改那一页）。
///
/// 其余（`reason`、要不要口令）由 `render_decrypt` 自己从
/// [`crate::markdown::template_page`] 再问一遍 —— 同一张表，答案一样。
fn decrypt_node(name: &str) -> Node {
    let page = format!("{TEMPLATE_NAME}:{name}");
    let mut node = Node::new(Template {
        name: "decrypt".to_string(),
        params: vec![
            ("page".to_string(), page.clone()),
            ("label".to_string(), name.to_string()),
            ("internal".to_string(), "yes".to_string()),
        ],
        // 底部是空的：解开之后摆一句"已解锁"，不凭空造内容
        body: String::new(),
    });
    node.children.push(wikilink::link_node(&page, &page));
    node
}

/// 说法本身有问题（模板套模板超层数）：只给一句说明，**不给**解锁框。
///
/// 那种场合输了口令问题还在，给个输入框是荒唐的。分工由
/// `a_usage_problem_is_not_given_an_unlock_box` 钉住。
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

    /// `::decrypt` 自己套自己：**不会**无限递归。
    ///
    /// 这里记过一次查探的结果，因为"要不要给它加一道深度限制"是反复被问的一件事，
    /// 而答案是**不用**（作者定的：用户自己写的嵌套不必再限一层）。理由有两条：
    ///
    /// 1. **字面嵌套的深度等于原文长度。** `::decrypt page=甲` 的底部就写着
    ///    `::decrypt page=甲`，那底下还能再写一层，但写第三层就得在原文里真的
    ///    写第三遍 —— 所以它是有限的，测出来两层就到"已解锁"了。
    /// 2. **真能无限的只有借道用户模板**（底部写 `::甲`，于是每一轮都重新展开甲），
    ///    而那条路每一圈都过一次 `expand_user_template`，`MAX_TEMPLATE_DEPTH`
    ///    在那里查 —— 全局那道护栏已经管住了。
    ///
    /// 所以 `dispatch::render_decrypt` 里**没有**自己的深度限制：加了只会把
    /// 作者写意的 `::decrypt page=甲` 里面再嵌 `::decrypt page=乙` 拦下来，
    /// 而那是正当的用法。
    #[test]
    fn decrypt_nesting_terminates_because_the_text_itself_is_finite() {
        // 甲 引用 甲，而 甲 是能读的
        let mut pages = HashMap::new();
        pages.insert(
            "甲".to_string(),
            TemplatePage::Ready("::decrypt page=甲\n".into()),
        );

        let html = render_with_pages("::甲\n", None, Some(Arc::new(pages)));
        // 底部是空的 → 一句"已解锁"，而不是又去解一次
        assert!(html.contains("已解锁"), "{html}");
        assert_eq!(
            html.matches("data-decrypt=\"yes\"").count(),
            0,
            "甲 没有锁，不该摆解锁框：{html}"
        );
    }

    /// 借道用户模板的那种递归：靠 `MAX_TEMPLATE_DEPTH` 收住，说清是"套太深"
    #[test]
    fn decrypt_recursion_through_a_user_template_is_cut_off_by_the_global_limit() {
        // 甲 = `::decrypt page=甲` + 底部再写 `::甲` —— 每一轮都重新展开甲
        let mut pages = HashMap::new();
        pages.insert(
            "甲".to_string(),
            TemplatePage::Ready("::decrypt page=甲\n\n::甲\n".into()),
        );

        let html = render_with_pages("::甲\n", None, Some(Arc::new(pages)));
        assert!(
            html.contains("模板嵌套超过"),
            "该被全局的深度限制收住并说清为什么：{html}"
        );
    }

    /// `::decrypt page=X` 是一个**能给人写**的模板：解开之后用底部内容替换它的位置。
    ///
    /// 这一条钉的是作者要的那件事本身。底部内容是一个 markdown 子页面 ——
    /// 与别的模板的块内容同一套做法（`scanner.rs` 第 113 行），所以里面能写字、
    /// 能列表、能再嵌模板。
    #[test]
    fn a_hand_written_decrypt_replaces_itself_with_the_body_once_unlocked() {
        // 甲 是**能读的**（没锁）→ 直接渲染底部，不摆框
        let mut pages = HashMap::new();
        pages.insert(
            "甲".to_string(),
            TemplatePage::Ready("::decrypt page=甲\n\n解开了。**这一段**是替代内容。\n".into()),
        );

        // 块正文要**缩进** —— 与所有模板同一套块边界规则（见 `scanner.rs` 抬头）
        let html = render_with_pages(
            "::decrypt page=甲\n  解开了。**这一段**是替代内容。\n",
            None,
            Some(Arc::new(pages)),
        );

        assert!(
            !html.contains("data-decrypt=\"yes\""),
            "没锁就不该摆框：{html}"
        );
        // 底部内容真的渲染出来了，而且是**解析过的**（`**这一段**` 变成 `<strong>`）
        assert!(html.contains("<strong>这一段</strong>"), "{html}");
        // 不该同时摆一句"已解锁"：有底部内容时它就是替代内容本身
        assert!(!html.contains("data-decrypt-done"), "{html}");
    }

    /// 底部为空 + 已经解锁 → 摆一句"已解锁"，**留在原位**
    #[test]
    fn an_unlocked_decrypt_with_no_body_leaves_a_marker_where_it_stands() {
        let mut pages = HashMap::new();
        pages.insert(
            "甲".to_string(),
            TemplatePage::Ready("随便什么内容\n".into()),
        );

        let html = render_with_pages("::decrypt page=甲\n", None, Some(Arc::new(pages)));

        assert!(html.contains(r#"data-decrypt-done="yes""#), "{html}");
        assert!(html.contains("已解锁"), "{html}");
        // 悄悄消失的话，正文会突然少一块 —— 人分不清是程序吃掉了还是自己写错了
        assert!(html.contains("class=\"decrypt decrypt--done\""), "{html}");
    }

    /// `page` 必填：没写就是用法错误，给一句说明（不是解锁框）
    #[test]
    fn decrypt_without_a_page_says_so_instead_of_pretending() {
        let html = render_with_pages("::decrypt\n", None, None);
        assert!(html.contains("缺少 page="), "{html}");
        assert!(!html.contains("data-decrypt=\"yes\""), "不该摆框：{html}");
    }

    /// `page` 指的那一页压根不存在 → 说清是"不存在"，不是"读不出来"
    #[test]
    fn decrypt_pointing_at_a_page_that_is_not_there_says_so() {
        let html = render_with_pages("::decrypt page=没有这一页\n", None, None);
        assert!(html.contains("不存在"), "{html}");
        assert!(!html.contains("data-decrypt=\"yes\""), "不该摆框：{html}");
    }

    /// **查看页面代码时保持原样** —— 这是作者点名的第二条。
    ///
    /// 解析器发现模板页读不出来时会"当作写了一个 `::decrypt`"，但那是**渲染期**的
    /// 替换：宿主那一篇的原文里仍然是 `::甲`，一个字都没动。
    ///
    /// 这条钉的就是这件事：渲染走一遍之后，**原文那个字符串本身**必须一模一样 ——
    /// 万一将来有人图省事把 `::decrypt page=甲` 写回页子里，这条会立刻炸。
    #[test]
    fn rendering_does_not_rewrite_the_source_it_was_given() {
        let source = String::from("::甲\n\n后面还有正文，别动它。\n");
        let before = source.clone();

        let mut pages = HashMap::new();
        pages.insert(
            "甲".to_string(),
            TemplatePage::Unreadable {
                reason: "这一页是加密存的".into(),
                protection: crate::storage::codec::Protection {
                    symmetric: true,
                    ..crate::storage::codec::Protection::plain()
                },
            },
        );
        let html = render_with_pages(&source, None, Some(Arc::new(pages)));

        assert!(
            html.contains("data-decrypt=\"yes\""),
            "渲染期该摆框：{html}"
        );
        // 渲染期摆了解锁框，而**原文里没有半个 `decrypt`**
        assert_eq!(source, before, "原文被改了");
        assert!(!source.contains("decrypt"), "{source}");
        assert!(source.contains("::甲"), "{source}");
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
