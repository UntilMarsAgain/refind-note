//! 块级规则本体：在 markdown-it 的块解析里认出 `::名字 …` 那一行，往下量缩进，收成一个节点。
//!
//! 三件事，各自有讲究：
//!
//! 1. **头**：`::名字 key=value …` 交给 [`parse`] 解析（引号与转义那里的活最多）；
//! 2. **块边界**：往后看每一行的缩进 —— 更深算块内、不更深即结束，**空行不直接结束**
//!    （见下面 `run` 里那段规则）；收上来的行按公共缩进削平，还原成块正文；
//! 3. **内容**：交给**整个解析器**再解析一遍，于是模板里能写 markdown、内部链接，
//!    也能再嵌模板。分节模板（[`SECTIONED`](super::stdlib::SECTIONED)）例外 ——
//!    块内容先按 `[标签]` 切成几节，各节各解析一遍，每节是一个 [`Section`]。
//!
//! 名字不在标准表里时，交给 [`expand`] 去用户命名空间找同名的模板页。

use super::expand::expand_user_template;
use super::parse::Template;
use super::stdlib;
use super::Section;
use markdown_it::parser::block::{BlockRule, BlockState};
use markdown_it::Node;

/// 块级规则：由 [`super::add`] 挂到解析器上，排在段落规则之前。
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

        // (相对块头的缩进, 行文本)：缩进要留着，块内容的层级靠它
        // ---- 块边界在这里定规则 ----
        //
        // 三条规则：更深算块内；不更深即结束；**空行不直接结束**，要往后看一行 ——
        // 后面还有更深的内容，这个空行才算块内。
        let mut body_lines: Vec<(usize, &str)> = Vec::new();
        let mut line = start + 1;
        while line < state.line_max {
            // 空行的 `line_indent` 是 0 而不是 -1，必须用 `is_empty` 单独判
            let blank = state.is_empty(line) || state.line_indent(line) < 0;
            if blank {
                // 空行：只有后面还有更深的内容时才算块内，否则它属于块外
                let next = (line + 1..state.line_max).find(|candidate| {
                    !state.is_empty(*candidate) && state.line_indent(*candidate) >= 0
                });
                match next {
                    Some(next) if state.line_indent(next) > marker_indent => {
                        body_lines.push((0, ""));
                        line += 1;
                        continue;
                    }
                    _ => break,
                }
            }
            if state.line_indent(line) <= marker_indent {
                break;
            }
            let indent = state.line_indent(line).max(0) as usize;
            body_lines.push((indent, state.get_line(line)));
            line += 1;
        }

        let body = rebuild_body(&body_lines);
        // `text=…` 给了就以它为准（`::banner` 那种一条横条，正文与参数二选一）。
        // 它也要能写 markdown，所以和块内容走同一条路。
        let source = match params.iter().find(|(key, _)| key == "text") {
            Some((_, text)) => text.clone(),
            None => body.clone(),
        };

        // **用户自定义模板**：名字不在内置表里，就去 `Template:` 命名空间找同名的页
        // （`::卡片` → `Template:卡片`）。找得到就把那一页的正文填好参数、当这一块的内容
        // 解析 —— 于是模板里照样能写 markdown、能嵌别的模板。
        //
        // 内置的优先：名字撞上时按内置的算（不然 `::quote` 会被某个同名页面顶掉）。
        if !stdlib::is_builtin(&name) {
            if let Some(node) = expand_user_template(&name, params.clone(), body.clone(), state) {
                return Some((node, line - start));
            }
        }

        let sectioned = stdlib::takes_sections(&name);
        let mut node = Node::new(Template { name, params, body });
        // 内容交给**整个解析器**再解析一遍：模板里因此可以写 markdown、内部链接，
        // 也可以再嵌模板（嵌套的 `::quote` 就是靠这一步成立的）。
        //
        // 代价记在这里，因为它是**已知的慢**：块规则在这一行重新进整个解析器，于是
        // 第 1 层的 `parse` 会再去处理第 2 层，第 2 层又处理第 3 层…… 嵌套 N 层时
        // 累计的代价随 N 超线性增长（实测每层约 ×1.95）。
        //
        // 实测（16 层，每层正文一行普通文字，**不涉及 `<markdown>`**）：
        // - 空正文：0.2 ms（与层数几乎无关）
        // - 每层一行字：29 ms（12 层） → 467 ms（16 层）
        //
        // 所以慢的触发条件是"**嵌套的 `::` 块里有内容**"，不是块嵌套本身。这一点与
        // `template::html` 无关 —— 它早就这样了。要治得从这里下手（例如别在块规则里
        // 重新进整个解析器），而不是在净化器那边加限制器 —— 加过，没用。
        //
        // `Node` 带 Drop，字段不能直接搬出来，所以用 `mem::take` 换走它的 children。
        if sectioned {
            // 分节模板：块内容先按 `[标签]` 切成几节，**各节各解析一遍** ——
            // 一节是一篇小文档，这才谈得上"这一节里写什么"（见 [`Section`]）
            for (label, text) in split_sections(&source) {
                let mut section = Node::new(Section { label });
                let mut parsed = state.md.parse(&text);
                section.children = std::mem::take(&mut parsed.children);
                node.children.push(section);
            }
        } else {
            let mut parsed = state.md.parse(&source);
            node.children = std::mem::take(&mut parsed.children);
        }
        Some((node, line - start))
    }
}

/// 拼回块内容：`(相对块头的缩进, 行文本)` → 去掉公共缩进后的文本。
///
/// **必须用 `state.line_indent` 的数值**，不能去数字符串开头的空格：
/// markdown-it 的 `get_line` 已经把行首空白吃掉了（"trimming initial spaces"），
/// 数出来永远是 0 —— 那样块内的层级会被悄悄抹平，嵌套的模板就坏了。
fn rebuild_body(lines: &[(usize, &str)]) -> String {
    let shared = lines
        .iter()
        .filter(|(_, text)| !text.trim().is_empty())
        .map(|(indent, _)| *indent)
        .min()
        .unwrap_or(0);
    lines
        .iter()
        .map(|(indent, text)| {
            if text.is_empty() {
                String::new()
            } else {
                format!("{}{}", " ".repeat(indent.saturating_sub(shared)), text)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// 把块内容按 `[标签]` 行切成几节：`(标签, 该节的正文)`。
///
/// 第一个标签**之前**的内容归到一个标签为空串的节里 —— 切分只管切，
/// 那一节算"两边都要"还是"这写法不对"，由各个模板自己定（见 [`stdlib::panels`]）。
fn split_sections(source: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for line in source.lines() {
        if let Some(label) = section_label(line) {
            out.push((label, String::new()));
            continue;
        }
        if out.is_empty() {
            // 还没有任何标签：空白丢掉，有字就先开一节（空标签）
            if line.trim().is_empty() {
                continue;
            }
            out.push((String::new(), String::new()));
        }
        let section = out.last_mut().expect("上面保证过至少开了一节");
        // 节首的空行不留（那是标签与正文之间的空档）；节内的空行留着，正文要用它分段
        if line.trim().is_empty() && section.1.trim().is_empty() {
            continue;
        }
        section.1.push_str(line);
        section.1.push('\n');
    }
    out
}

/// `[标签]` —— 分节模板里一节的开头。
///
/// 两条都认：**整行**就是一对中括号（正文里一句 `[注] 说明` 不该被当成新的一节），
/// 而且**从行首开始**（块内容此时已去掉公共缩进，所以本体的一行就在第 0 列，
/// 缩进更深的是**里层**的内容）。少了第二条，嵌套的选项卡会被外层吃掉：
/// 里层的 `[内]` 也当成外层的节了。
fn section_label(line: &str) -> Option<String> {
    let inner = line.strip_prefix('[')?.trim_end().strip_suffix(']')?;
    let label = inner.trim();
    if label.is_empty() || label.contains(['[', ']']) {
        return None;
    }
    Some(label.to_string())
}

#[cfg(test)]
mod tests {
    use crate::markdown::render;

    // 这一组测的是**块边界怎么划**，所以占位名必须挑一个**不是内置模板**的
    // （`::外层` / `::inner`）：内置模板各有各的渲染器，`::note` 这类名字一旦
    // 进了注册表，块就渲染成了那个样子，边界反而测不出来了。
    // 曾经这几个测试拿 `::note` 当占位名，加提示框模板那天它们就红了 —— 根因在此。

    #[test]
    fn indentation_separates_inside_from_outside() {
        let html = render("::外层\n  块内\n块外\n");
        assert!(html.contains("块内"), "{html}");
        assert!(html.contains("<p>块外</p>"), "{html}");
    }

    #[test]
    fn deeper_levels_stay_inside() {
        let html = render("::外层\n  第一层\n    ::inner\n      第二层\n");
        assert!(html.contains("::inner"), "{html}");
        assert!(html.contains("第二层"), "{html}");
    }

    #[test]
    fn blank_line_inside_stays_inside() {
        let html = render("::外层\n  第一段\n\n  第二段\n");
        assert!(html.contains("第一段"), "{html}");
        assert!(html.contains("第二段"), "{html}");
        assert!(!html.contains("<p>第二段</p>"), "第二段仍应在块内：{html}");
    }

    #[test]
    fn blank_line_before_unindented_text_ends_the_block() {
        let html = render("::外层\n  块内\n\n块外\n");
        assert!(html.contains("块内"), "{html}");
        assert!(html.contains("<p>块外</p>"), "{html}");
    }

    #[test]
    fn dedent_keeps_relative_structure() {
        let html = render("::外层\n    两空格缩进之下\n      再深一层\n");
        assert!(html.contains("两空格缩进之下"), "{html}");
        assert!(html.contains("再深一层"), "{html}");
    }

    /// 空行**不直接结束块**：后面还有更深的内容，它就算块内。
    ///
    /// 这条是块边界规则里最容易走样的地方（"空行即结束"看着更直觉），
    /// 所以在这里钉死，改渲染规则时会先撞上它。
    #[test]
    fn blank_line_does_not_end_the_block_by_itself() {
        let html = render("::quote\n  第一段\n\n  第二段\n块外\n");
        assert!(html.contains("第一段"), "{html}");
        assert!(html.contains("第二段"), "{html}");
        // 两段都应当在同一个引用块里
        assert_eq!(html.matches("<blockquote").count(), 1, "{html}");
        let before = &html[..html.find("第二段").unwrap()];
        let depth = before.matches("<blockquote").count() - before.matches("</blockquote>").count();
        assert_eq!(depth, 1, "第二段应当仍在块内：{html}");
        // 不再是更深的那一行才结束
        assert!(html.contains("<p>块外</p>"), "{html}");
    }

    #[test]
    fn body_is_parsed_by_the_whole_parser() {
        // 模板内容里的 markdown 与内部链接都要生效
        let html = render("::quote\n  这是**强调**与[[目标]]\n");
        assert!(html.contains("<strong>强调</strong>"), "{html}");
        assert!(html.contains("wikilink"), "{html}");
    }

    #[test]
    fn unknown_template_renders_a_box() {
        let html = render("::还没有的模板 标题=\"含 空格\" flag\n  内容一行\n");
        assert!(html.contains("template--unknown"), "{html}");
        assert!(html.contains("未知模板"), "{html}");
        assert!(
            html.contains(r#"<code class="template__name">还没有的模板</code>"#),
            "{html}"
        );
        assert!(html.contains("标题=含 空格"), "{html}");
        assert!(html.contains("<pre><code>内容一行</code></pre>"), "{html}");
    }
}
