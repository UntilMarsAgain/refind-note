//! 模板块：`::名字 参数=值 …` 开一个块，块的边界由**缩进**划出。
//!
//! ```text
//! ::name key=value key=value
//!   content
//! ```
//!
//! 分三层，本文件目前只管第一层：
//!
//! 1. **认出块并拆出头**（这里）：一行以 `::` 开头就是块的开头，随后取名字与若干
//!    `key=value` 参数；接着**缩进比头更深**的行是块内容，遇到不更深的行就结束 ——
//!    缩进就是块内与块外的唯一分界，不需要结束标记。
//! 2. **按名字分发**：把块交给登记在册的渲染器。这块能力还没有，登记表是空的。
//! 3. **兜底**：原样当代码块显示。这是**暂时**的行为 —— 先让第一层做对、看得见，
//!    而不是一次写完半成品。
use markdown_it::parser::block::{BlockRule, BlockState};
use markdown_it::{MarkdownIt, Node, NodeValue, Renderer};

/// 一个模板块：头（名字 + 参数）与块内容。
#[derive(Debug, Clone, PartialEq)]
pub struct Template {
    /// `::` 后面的名字（分发就用它）
    pub name: String,
    /// `key=value` 参数；只写了 `key` 的开关式参数，值是空串
    pub params: Vec<(String, String)>,
    /// 块内容，已去掉公共缩进
    pub body: String,
    /// 整块原文（含头），兜底渲染用
    pub raw: String,
}

impl Template {
    /// 解析头一行：`::名字 key=value …`。不是头就返回 `None`。
    ///
    /// 名字必须紧跟在 `::` 之后，且不能含 `=` / `:` —— 那两个字符属于参数与命名空间，
    /// 出现在名字里几乎一定是写错了。宁可退回普通段落，也不要认出一个坏块。
    pub fn parse_header(line: &str) -> Option<(String, Vec<(String, String)>)> {
        let rest = line.trim_start().strip_prefix("::")?;
        let mut tokens = rest.split_whitespace();
        let name = tokens.next()?.to_string();
        if name.contains('=') || name.contains(':') {
            return None;
        }
        let params = tokens
            .map(|token| match token.split_once('=') {
                Some((key, value)) => (key.to_string(), value.to_string()),
                None => (token.to_string(), String::new()),
            })
            .collect();
        Some((name, params))
    }

    /// 取一个参数（下一步的渲染器要用；现在还没有渲染器，所以暂时没人调）
    #[allow(dead_code)]
    pub fn param(&self, key: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
    }
}

impl NodeValue for Template {
    fn render(&self, _node: &Node, fmt: &mut dyn Renderer) {
        // 还没有登记任何渲染器：原样当代码块显示。
        // `fmt.text` 会做 HTML 转义，所以笔记里的尖括号不会漏进页面。
        fmt.cr();
        fmt.open("pre", &[]);
        fmt.open("code", &[("class", "language-template".to_string())]);
        fmt.text(&self.raw);
        fmt.close("code");
        fmt.close("pre");
        fmt.cr();
    }
}

/// 一行开头有多少个空白字符（只数空格与制表符）。
///
/// 不直接用 `trim_start` 的长度差：那会把全角空格之类的 Unicode 空白也算进去，
/// 而按字节切字符串是要出事的（本项目在地址解析上吃过一次）。
fn indent_of(line: &str) -> usize {
    line.chars().take_while(|c| *c == ' ' || *c == '\t').count()
}

/// 去掉块内容的公共缩进：作者写 2 空格还是 4 空格都行，块内的相对层级保持不变。
fn dedent(lines: &[&str]) -> String {
    let shared = lines
        .iter()
        .filter(|line| !line.trim().is_empty())
        .map(|line| indent_of(line))
        .min()
        .unwrap_or(0);
    lines
        .iter()
        .map(|line| line.chars().skip(shared).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn add(md: &mut MarkdownIt) {
    // 必须排在段落规则之前：否则 `::名字` 会先被当成一行普通文字收走
    md.block
        .add_rule::<TemplateScanner>()
        .before::<markdown_it::plugins::cmark::block::paragraph::ParagraphScanner>()
        .after_all();
}

#[doc(hidden)]
pub struct TemplateScanner;

impl BlockRule for TemplateScanner {
    fn run(state: &mut BlockState) -> Option<(Node, usize)> {
        let start = state.line;
        let marker_indent = state.line_indent(start);
        if marker_indent < 0 {
            return None;
        }
        let (name, params) = Template::parse_header(state.get_line(start))?;

        let mut body_lines: Vec<&str> = Vec::new();
        let mut line = start + 1;
        while line < state.line_max {
            // 空行的 `line_indent` 是 0 而不是 -1，必须用 `is_empty` 单独判
            let blank = state.is_empty(line) || state.line_indent(line) < 0;
            if blank {
                // 空行：只有后面还有更深的内容时才算块内，否则它属于块外
                let next = (line + 1..state.line_max)
                    .find(|candidate| !state.is_empty(*candidate) && state.line_indent(*candidate) >= 0);
                match next {
                    Some(next) if state.line_indent(next) > marker_indent => {
                        body_lines.push("");
                        line += 1;
                        continue;
                    }
                    _ => break,
                }
            }
            if state.line_indent(line) <= marker_indent {
                break;
            }
            body_lines.push(state.get_line(line));
            line += 1;
        }

        // 原文照收：兜底渲染要能一字不差地看到作者写了什么
        let raw = (start..line)
            .map(|index| state.get_line(index))
            .collect::<Vec<_>>()
            .join("\n");
        let node = Node::new(Template {
            name,
            params,
            body: dedent(&body_lines),
            raw,
        });
        Some((node, line - start))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::render;

    #[test]
    fn parses_name_and_params() {
        let (name, params) = Template::parse_header("::note key=value flag 另一个=值").unwrap();
        assert_eq!(name, "note");
        assert_eq!(
            params,
            vec![
                ("key".to_string(), "value".to_string()),
                // 只写 key 的开关式参数也认
                ("flag".to_string(), String::new()),
                ("另一个".to_string(), "值".to_string()),
            ]
        );
    }

    #[test]
    fn rejects_lines_that_are_not_headers() {
        // 只有 `::` 没有名字
        assert!(Template::parse_header("::").is_none());
        // 名字里不该有 `=` 或 `:`
        assert!(Template::parse_header("::=x").is_none());
        assert!(Template::parse_header("::a:b").is_none());
        // 不是以 `::` 开头
        assert!(Template::parse_header(":name").is_none());
        assert!(Template::parse_header("普通一行").is_none());
    }

    #[test]
    fn block_content_is_marked_as_code_for_now() {
        let html = render("::note key=value\n  正文一行\n");
        assert!(html.contains("language-template"), "{html}");
        assert!(html.contains("::note key=value"), "{html}");
        assert!(html.contains("正文一行"), "{html}");
    }

    #[test]
    fn indentation_separates_inside_from_outside() {
        let html = render("::note\n  块内\n块外\n");
        assert!(html.contains("块内"), "{html}");
        // 不更深的那一行在块外，仍然是一段普通文字
        assert!(html.contains("<p>块外</p>"), "{html}");
    }

    #[test]
    fn deeper_levels_stay_inside() {
        let html = render("::note\n  第一层\n    ::inner\n      第二层\n");
        // 头行之后的更深行都属于块内容，包括看起来像头的那些
        assert!(html.contains("::inner"), "{html}");
        assert!(html.contains("第二层"), "{html}");
    }

    #[test]
    fn blank_line_inside_stays_inside() {
        let html = render("::note\n  第一段\n\n  第二段\n");
        assert!(html.contains("第一段"), "{html}");
        assert!(html.contains("第二段"), "{html}");
        assert!(!html.contains("<p>第二段</p>"), "第二段仍应在块内：{html}");
    }

    #[test]
    fn blank_line_before_unindented_text_ends_the_block() {
        let html = render("::note\n  块内\n\n块外\n");
        assert!(html.contains("块内"), "{html}");
        assert!(html.contains("<p>块外</p>"), "{html}");
    }

    #[test]
    fn dedent_keeps_relative_structure() {
        let html = render("::note\n    两空格缩进之下\n      再深一层\n");
        assert!(html.contains("两空格缩进之下"), "{html}");
        assert!(html.contains("再深一层"), "{html}");
    }

}
