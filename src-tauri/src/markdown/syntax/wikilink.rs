//! 内部链接：`[[目标|显示文字]]`
//!
//! 顺序遵循 MediaWiki：**目标在前、显示文字在后**，`|显示文字` 可以省略
//! （省略时显示文字就是目标本身）。
//! 注意别写反成 `[[显示文字|目标]]`。
//!
//! 渲染成不带 `href` 的 `<a class="wikilink" data-doc="…">`：
//! href 故意留空，否则会被前端「a[href] 一律当外链打开」那段逻辑吞掉。

use markdown_it::parser::inline::{InlineRule, InlineState, Text};
use markdown_it::{MarkdownIt, Node, NodeValue, Renderer};

/// 目标文档名挂在节点上，渲染时落成 data-doc
#[derive(Debug)]
pub struct WikiLink {
    pub doc: String,
}

impl NodeValue for WikiLink {
    fn render(&self, node: &Node, fmt: &mut dyn Renderer) {
        let mut attrs = node.attrs.clone();
        attrs.push(("class", "wikilink".into()));
        attrs.push(("data-doc", self.doc.clone()));

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

        // 不要自己推进 state.pos：tokenize 会用这里返回的长度去推进
        let consumed = 2 + end + 2;
        let mut node = Node::new(WikiLink {
            doc: doc.to_owned(),
        });
        node.children.push(Node::new(Text {
            content: text.to_owned(),
        }));

        Some((node, consumed))
    }
}

#[cfg(test)]
mod tests {
    use crate::markdown::render;

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
}
