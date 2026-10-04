//   Refind Note is a note-taking software.
//   Copyright (C) 2026 Until Mars Again
//
//   This program is free software: you can redistribute it and/or modify
//   it under the terms of the GNU Affero General Public License as published by
//   the Free Software Foundation, either version 3 of the License, or
//   (at your option) any later version.
//
//   This program is distributed in the hope that it will be useful,
//   but WITHOUT ANY WARRANTY; without even the implied warranty of
//   MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
//   GNU Affero General Public License for more details.
//
//   You should have received a copy of the GNU Affero General Public License
//   along with this program.  If not, see <http://www.gnu.org/licenses/>.

//! 数学公式：行内 `$…$`、独立一行（或独立一段）的 `$$…$$`
//!
//! 这里只**认出**公式，把 TeX 原文放进 `data-tex`；真正的排版（KaTeX）在前端 ——
//! 后端不引 JS 引擎，而公式要跟着主题、字号、缩放进正文，本来就该在前端落地。
//! 没渲染之前元素里显示的是**公式原文**（不是空白），取不到 KaTeX 也读得下去。
//!
//! 识别的规矩，取"宁可漏认、也别把正文里的美元符号吞掉"：
//!
//! - `$…$`：单行；两个 `$` 都**紧挨着**内容（`$ 5 $` 不算），中间不能再有 `$`；
//! - `$$…$$`：同样单行。多行的整块公式用 [`super::template`] 的 `::math` 模板。
//!
//! 代码块与行内代码里的 `$` 不受影响 —— 那是更外层的规则先接走的。

use markdown_it::parser::inline::{InlineRule, InlineState, Text};
use markdown_it::{MarkdownIt, Node, NodeValue, Renderer};

/// 一段公式
#[derive(Debug, Clone, PartialEq)]
pub struct Math {
    /// TeX 原文
    pub tex: String,
    /// 是不是独立成行的"行间公式"（KaTeX 的 display 模式）
    pub display: bool,
}

impl NodeValue for Math {
    fn render(&self, node: &Node, fmt: &mut dyn Renderer) {
        let mut attrs = node.attrs.clone();
        attrs.push((
            "class",
            if self.display {
                "math math--display".to_string()
            } else {
                "math math--inline".to_string()
            },
        ));
        // 原文留一份在属性上：前端排版完会用元素内容换掉，重排时要拿得回来
        attrs.push(("data-tex", self.tex.clone()));

        fmt.open("span", &attrs);
        // 排版之前看到的就是原文
        fmt.text(&self.tex);
        fmt.close("span");
    }
}

pub fn add(md: &mut MarkdownIt) {
    md.inline.add_rule::<MathScanner>();
}

#[doc(hidden)]
pub struct MathScanner;

impl InlineRule for MathScanner {
    const MARKER: char = '$';

    fn run(state: &mut InlineState) -> Option<(Node, usize)> {
        // 只看这一行剩下的部分（`pos_max` 就是行尾）：公式不跨行
        let rest = &state.src[state.pos..state.pos_max];

        // `$$…$$` 先认：它更具体，不然会被行内那条吃掉两个 `$`
        if let Some(body) = rest.strip_prefix("$$") {
            let end = body.find("$$")?;
            let tex = body[..end].trim();
            if tex.is_empty() {
                return None;
            }
            // `$$` + 内容 + `$$`
            return Some((node(tex, true), 4 + end));
        }

        let body = rest.strip_prefix('$')?;
        let end = body.find('$')?;
        let tex = &body[..end];

        // 贴边有空白就不认：`价格 $5 到 $9` 里的那对 `$` 不能被当成公式
        if tex.is_empty()
            || tex.starts_with(char::is_whitespace)
            || tex.ends_with(char::is_whitespace)
        {
            return None;
        }

        // `$` + 内容 + `$`
        Some((node(tex, false), 2 + end))
    }
}

fn node(tex: &str, display: bool) -> Node {
    let mut node = Node::new(Math {
        tex: tex.to_string(),
        display,
    });
    // 节点自身不带文字：内容由渲染器写成属性 + 原文
    node.children.push(Node::new(Text {
        content: String::new(),
    }));
    node
}

#[cfg(test)]
mod tests {
    use crate::markdown::render;

    #[test]
    fn inline_math_keeps_the_source_for_the_frontend() {
        let html = render("爱因斯坦说 $E = mc^2$ 是这样。\n");
        assert!(html.contains(r#"class="math math--inline""#), "{html}");
        assert!(html.contains(r#"data-tex="E = mc^2""#), "{html}");
        // 排版之前看到的是原文
        assert!(html.contains(">E = mc^2</span>"), "{html}");
    }

    #[test]
    fn double_dollars_ask_for_display_mode() {
        let html = render("$$\\int_0^1 x^2 dx$$\n");
        assert!(html.contains("math--display"), "{html}");
        assert!(html.contains(r#"data-tex="\int_0^1 x^2 dx""#), "{html}");
    }

    /// 钱的写法不能被吞掉 —— 这是最容易误伤的一类
    #[test]
    fn prices_stay_prose() {
        for src in [
            "这本书 $5 到 $9。\n",
            "价格是 $ 5 元\n",
            "只有一个 $ 符号\n",
        ] {
            let html = render(src);
            assert!(!html.contains("math--"), "{src:?} 不该被当成公式：{html}");
        }
    }

    /// 代码里的 `$` 原样留着
    #[test]
    fn code_keeps_its_dollars() {
        let html = render("`echo $HOME` 与\n\n```sh\necho $HOME\n```\n");
        assert!(!html.contains("math--"), "{html}");
        assert!(html.contains("$HOME"), "{html}");
    }

    /// 特殊字符要被转义，不能因为写着公式就把 HTML 放进来
    #[test]
    fn angle_brackets_are_escaped() {
        let html = render("$a < b > c$\n");
        assert!(!html.contains("<b>"), "{html}");
        assert!(html.contains("&lt;"), "{html}");
    }
}
