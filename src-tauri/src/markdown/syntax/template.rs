//! 模板块：`::名字 参数=值 …` 开一个块，块的边界由**缩进**划出。
//!
//! ```text
//! ::name key=value key=value
//!   content
//! ```
//!
//! 分三步，都在这里：
//!
//! 1. **认出块并拆出头**：一行以 `::` 开头就是块的开头，随后取名字与若干 `key=value`
//!    参数；接着**缩进比头更深**的行是块内容，遇到不更深的行就结束 —— 缩进就是块内与
//!    块外的唯一分界，不需要结束标记。
//! 2. **按名字分发**：见 [`TEMPLATES`] 那张表。有什么模板只写在那一个地方。
//! 3. **查不到就报错**：[`render_unknown`] 渲染一个框，把名字与参数原样列出，内容仍用
//!    代码块裹住 —— 写得再错也不会让内容静静消失。
//!
//! 名字、key、value 都可以用引号保护其中的空格；引号里用反斜杠保护引号本身。
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
}

impl Template {
    /// 解析头一行：`::名字 key=value …`。不是头就返回 `None`。
    ///
    /// 名字必须紧跟在 `::` 之后，且不能含 `=` / `:` —— 那两个字符属于参数与命名空间，
    /// 出现在名字里几乎一定是写错了。宁可退回普通段落，也不要认出一个坏块。
    pub fn parse_header(line: &str) -> Option<(String, Vec<(String, String)>)> {
        let rest = line.trim_start().strip_prefix("::")?;
        let mut tokens = tokenize(rest)?.into_iter();
        let name = tokens.next()?;
        if name.is_empty() || name.contains('=') || name.contains(':') {
            return None;
        }
        // `=` 按**第一个**切：`key="a=b"` 的值就是 `a=b`。
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

/// 把一行切成 token：空白分隔，但**引号里的空白不算分隔**。
///
/// 单引号与双引号都可以；引号内用反斜杠保护引号本身（`\"`）与反斜杠（`\\`），
/// 引号外反斜杠没有特殊含义（路径里到处都是反斜杠，不该在这里被吃掉）。
///
/// 引号没有配对就整行走不通：返回 `None`，这一行退回普通段落 —— 宁可当普通文字，
/// 也不要猜作者想要什么。
fn tokenize(line: &str) -> Option<Vec<String>> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut started = false;
    let mut quote: Option<char> = None;
    let mut chars = line.chars();

    while let Some(ch) = chars.next() {
        match quote {
            Some(closing) => {
                if ch == '\\' {
                    // 只认这两个转义：引号与反斜杠本身
                    match chars.next() {
                        Some('\\') => current.push('\\'),
                        Some(next) if next == closing => current.push(closing),
                        Some(next) => {
                            current.push('\\');
                            current.push(next);
                        }
                        None => return None,
                    }
                } else if ch == closing {
                    quote = None;
                } else {
                    current.push(ch);
                }
            }
            None => match ch {
                '\'' | '"' => {
                    quote = Some(ch);
                    // `""` 是一个空值的 token，不是没有 token
                    started = true;
                }
                ch if ch.is_whitespace() => {
                    if started {
                        tokens.push(std::mem::take(&mut current));
                        started = false;
                    }
                }
                ch => {
                    current.push(ch);
                    started = true;
                }
            },
        }
    }

    if quote.is_some() {
        return None;
    }
    if started {
        tokens.push(current);
    }
    Some(tokens)
}

/// 一个模板渲染器：拿到解析好的块，往 `fmt` 里写 HTML。
pub type TemplateRenderer = fn(&Template, &mut dyn Renderer);

/// 模板分发表：名字 → 渲染器。
///
/// 与指令表同样的规矩：**有什么模板只写在这一个地方**，别处不再各自认一遍名字。
/// 表还是空的（一个真正的模板都还没有），所以现在每块都会走 [`render_unknown`]。
pub static TEMPLATES: &[(&str, TemplateRenderer)] = &[];

/// 按名字分发；查不到就交给"未知模板"。
pub fn render_template(template: &Template, fmt: &mut dyn Renderer) {
    match TEMPLATES.iter().find(|(name, _)| *name == template.name) {
        Some((_, render)) => render(template, fmt),
        None => render_unknown(template, fmt),
    }
}

/// 名字查不到时的兜底：渲染一个框，把名字与参数原样列出，内容仍用代码块裹住。
///
/// 这是**给作者看的错误提示**，不是"不认识就静静丢掉"。参数用等号写法回显，
/// 所以从输出上能一眼看出解析成了什么 —— 引号去了哪儿、空格是否保住。
fn render_unknown(template: &Template, fmt: &mut dyn Renderer) {
    fmt.cr();
    fmt.open(
        "div",
        &[("class", "template template--unknown".to_string())],
    );
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
    fmt.open("pre", &[]);
    fmt.open("code", &[]);
    fmt.text(&template.body);
    fmt.close("code");
    fmt.close("pre");
    fmt.cr();
    fmt.close("div");
    fmt.cr();
}

impl NodeValue for Template {
    fn render(&self, _node: &Node, fmt: &mut dyn Renderer) {
        render_template(self, fmt);
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

        let node = Node::new(Template {
            name,
            params,
            body: dedent(&body_lines),
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
    fn quotes_protect_spaces_and_escapes() {
        let (name, params) =
            Template::parse_header(r#"::"两 个词" 标题="含 空格" 引号="他说\"好\"" flag"#).unwrap();
        assert_eq!(name, "两 个词");
        assert_eq!(
            params,
            vec![
                ("标题".to_string(), "含 空格".to_string()),
                ("引号".to_string(), "他说\"好\"".to_string()),
                // 引号不改变参数的性质：仍然可以只写 key
                ("flag".to_string(), String::new()),
            ]
        );
    }

    #[test]
    fn unterminated_quote_is_not_a_header() {
        assert!(Template::parse_header("::note key=\"没关上").is_none());
    }

    #[test]
    fn backslash_outside_quotes_is_literal() {
        // 引号外反斜杠没有特殊含义：路径不该在这里被吃掉
        let (_, params) = Template::parse_header(r"::note path=C:\笔记").unwrap();
        assert_eq!(params, vec![("path".to_string(), r"C:\笔记".to_string())]);
    }

    #[test]
    fn unknown_template_renders_a_box() {
        let html = render("::还没有的模板 标题=\"含 空格\" flag\n  内容一行\n");
        assert!(html.contains("template--unknown"), "{html}");
        assert!(html.contains("未知模板"), "{html}");
        // 名字与参数原样列出（参数回显成等号写法，一眼看出解析成了什么）
        assert!(
            html.contains(r#"<code class="template__name">还没有的模板</code>"#),
            "{html}"
        );
        assert!(html.contains("标题=含 空格"), "{html}");
        assert!(html.contains("flag"), "{html}");
        // 内容仍用代码块裹住，不会静静消失
        assert!(
            html.contains("<pre><code>内容一行</code></pre>"),
            "{html}"
        );
    }

    #[test]
    fn dispatch_table_has_unique_names() {
        let mut names: Vec<&str> = TEMPLATES.iter().map(|(name, _)| *name).collect();
        let total = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(total, names.len(), "模板表里有重名");
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
