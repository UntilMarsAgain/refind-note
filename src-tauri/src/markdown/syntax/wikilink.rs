//! 内部链接：`[[目标|显示文字]]`
//!
//! 顺序遵循 MediaWiki：**目标在前、显示文字在后**，`|显示文字` 可以省略
//! （省略时显示文字就是目标本身）。
//! 注意别写反成 `[[显示文字|目标]]`。
//!
//! 渲染成 `<a class="wikilink" …>`：**普通内部链接故意不带 href**，否则会被前端
//! 「a[href] 一律当外链打开」那段逻辑吞掉；跨站（interwiki）链接反过来 —
//! 它本来就是外链，给出真实 `href` 并挂 `data-interwiki`，由前端当外链打开、由 CSS 画绿链。
//!
//! 目标是否存在（红链 / 蓝链）由后端判定：从 [`crate::markdown::current_resolver`]
//! 取当次渲染注入的解析器（见 [`crate::markdown::render_with`]）。取不到解析器时
//! 只输出 `data-doc`。

use crate::markdown::current_resolver;
use crate::title::Resolved;
use markdown_it::parser::inline::{InlineRule, InlineState, Text};
use markdown_it::{MarkdownIt, Node, NodeValue, Renderer};

/// 目标文档名挂在节点上，渲染时落成 data-* 属性
#[derive(Debug)]
pub struct WikiLink {
    /// 笔记里原样写的目标，便于排障与回退
    pub doc: String,
    /// 解析结果；没有注入解析器时为 None
    pub resolved: Option<Resolved>,
}

impl NodeValue for WikiLink {
    fn render(&self, node: &Node, fmt: &mut dyn Renderer) {
        let mut attrs = node.attrs.clone();
        attrs.push(("class", "wikilink".into()));
        attrs.push(("data-doc", self.doc.clone()));

        if let Some(resolved) = &self.resolved {
            attrs.push(("data-key", resolved.key.clone()));
            attrs.push(("data-title", resolved.title.clone()));
            match &resolved.url {
                // 跨站链接：给真实地址（前端按外链打开），绿链由 CSS 上色。
                // 它不参与"红链/蓝链"的判断 —— 那是本仓库有没有这一页的事。
                Some(url) => {
                    attrs.push(("href", url.clone()));
                    attrs.push(("data-interwiki", "true".into()));
                }
                None => attrs.push((
                    "data-missing",
                    if resolved.exists { "false" } else { "true" }.into(),
                )),
            }
        }

        fmt.open("a", &attrs);
        fmt.contents(&node.children);
        fmt.close("a");
    }
}

pub fn add(md: &mut MarkdownIt) {
    md.inline.add_rule::<WikiLinkScanner>();
}

#[doc(hidden)]
pub struct WikiLinkScanner;

impl InlineRule for WikiLinkScanner {
    const MARKER: char = '[';

    fn run(state: &mut InlineState) -> Option<(Node, usize)> {
        let body = state.src[state.pos..state.pos_max].strip_prefix("[[")?;
        let end = body.find("]]")?;
        let inner = &body[..end];

        // MediaWiki 顺序：目标在前，显示文字在后
        let (doc, text) = match inner.split_once('|') {
            Some((doc, text)) => (doc, text),
            None => (inner, inner),
        };

        // 目标或显示文字为空都不认，交回普通文本处理
        if doc.is_empty() || text.is_empty() {
            return None;
        }

        // 红链 / 蓝链在这里定：目标是否存在于仓库
        let resolved = current_resolver().and_then(|resolver| resolver.resolve(doc));

        // 不要自己推进 state.pos：tokenize 会用这里返回的长度去推进
        let consumed = 2 + end + 2;
        let mut node = Node::new(WikiLink {
            doc: doc.to_owned(),
            resolved,
        });
        node.children.push(Node::new(Text {
            content: text.to_owned(),
        }));

        Some((node, consumed))
    }
}

#[cfg(test)]
mod tests {
    use crate::markdown::{render, render_with};
    use crate::title::{LinkResolver, NamespaceTable};
    use std::collections::HashSet;
    use std::sync::Arc;

    #[test]
    fn target_first_display_second() {
        // [[目标|显示文字]]
        let html = render("[[目标文档|显示文字]]\n");
        assert!(html.contains(r#"class="wikilink""#), "{html}");
        assert!(html.contains(r#"data-doc="目标文档""#), "{html}");
        assert!(html.contains(">显示文字</a>"), "{html}");
        // 不能带 href，否则前端会把它当外链丢给系统浏览器
        assert!(!html.contains(r#"href="#), "{html}");
    }

    #[test]
    fn display_text_defaults_to_target() {
        let html = render("[[同名文档]]\n");
        assert!(html.contains(r#"data-doc="同名文档""#), "{html}");
        assert!(html.contains(">同名文档</a>"), "{html}");
    }

    #[test]
    fn broken_links_fall_back_to_text() {
        // 未闭合、空目标、空显示文字，都不该被当成内部链接
        for src in ["[[未闭合\n", "[[|只有显示文字]]\n", "[[只有目标|]]\n"] {
            let html = render(src);
            assert!(!html.contains("wikilink"), "{src:?} 不该被解析: {html}");
        }
    }

    #[test]
    fn stays_literal_inside_code_span() {
        let html = render("`[[a|b]]`\n");
        assert!(!html.contains("wikilink"), "{html}");
    }

    /// 不带解析器时只输出 data-doc，不该出现红/蓝标记
    #[test]
    fn without_resolver_there_is_no_red_or_blue() {
        let html = render("[[目标]]\n");
        assert!(!html.contains("data-missing"), "{html}");
        assert!(!html.contains("data-key"), "{html}");
    }

    /// 注入解析器后，红链与蓝链必须被标出来
    #[test]
    fn resolver_marks_red_and_blue_links() {
        let mut keys = HashSet::new();
        keys.insert("0:存在的条目".to_string());
        let resolver = LinkResolver::new(
            Arc::new(NamespaceTable::builtin()),
            Arc::new(keys),
            true,
            None,
        );

        let html = render_with("[[存在的条目]] 与 [[没有的条目]]\n", Some(&resolver));
        assert!(html.contains(r#"data-key="0:存在的条目""#), "{html}");
        assert!(html.contains(r#"data-missing="false""#), "{html}");
        assert!(html.contains(r#"data-missing="true""#), "{html}");
    }
}