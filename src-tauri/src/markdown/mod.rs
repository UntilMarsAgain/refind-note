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

//! Markdown 解析：插件注册、自定义语法挂载，以及 markdown → HTML 的入口。
//!
//! 解析器只构建一次（见 [`MARKDOWN`]）；解析本身取 `&self`，可复用。
//!
//! 这一层不知道存储：目标是否存在，由调用方在渲染前通过
//! [`crate::vault::target::Resolver`] 传进来，再经 [`render_with`] 注入当次渲染。

pub mod syntax;

use crate::vault::target::Resolver;
use markdown_it::MarkdownIt;
use std::cell::RefCell;
use std::sync::LazyLock;

/// 给标题生成 id，供页内跳转使用。
///
/// 保留字母数字（中日韩字符也算），其余字符归并为 '-'，并去掉首尾多余的 '-'。
/// 刻意不用 heading_anchors 自带的 `simple_slugify_fn`：它的文档明确写着
/// "added for testing and demonstration purposes only"。
fn slugify_heading(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pending_dash = false;

    for ch in text.chars() {
        if ch.is_alphanumeric() {
            if pending_dash && !out.is_empty() {
                out.push('-');
            }
            pending_dash = false;
            out.extend(ch.to_lowercase());
        } else {
            pending_dash = true;
        }
    }

    out
}

thread_local! {
    /// 当前渲染所用的链接解析器。
    ///
    /// 为什么不用解析器的 `ext` 集合：那是**解析器全局唯一**的，而渲染状态必须是
    /// **每次调用各自一份**——两个线程同时渲染时，一个的「用完清空」会把另一个
    /// 正在进行的渲染打断（并行跑测试就复现了）。`parse(&self, src)` 又没有 env
    /// 参数，所以用线程局部变量承载：解析本身是同步的，规则必然跑在同一个线程上。
    static CURRENT_RESOLVER: RefCell<Option<Resolver>> = const { RefCell::new(None) };

    /// 现在展开到第几层（用户模板可以互相嵌入，得防着转圈）
    static TEMPLATE_DEPTH: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };

    /// 当前渲染拿得到的**模板页正文**（`Template:` 命名空间里那几页）：名字 → 正文。
    ///
    /// 与解析器同一个理由：`Template::render` 只拿得到节点，够不着仓库 ——
    /// 而 `::html src="某张卡片"` 要的正是"另一个页面的正文"。
    static CURRENT_TEMPLATE_PAGES: RefCell<Option<TemplatePages>> = const { RefCell::new(None) };
}

/// 一页模板页在**这一刻**的样子
#[derive(Debug, Clone, PartialEq)]
pub enum TemplatePage {
    /// 取到了：正文
    Ready(String),
    /// 有这一页，但这一刻读不出来（没解锁、或者坏了）—— 里面是一句给人看的原因
    Unreadable(String),
}

/// 模板页的表：**小写页面名 → 那一页**。
///
/// 由仓库那一层在渲染前备好（见 `vault::notes` 里 `render_html` 旁边那段说明）。
pub type TemplatePages = std::sync::Arc<std::collections::HashMap<String, TemplatePage>>;

/// 取 `src=` 指向的那一页。
///
/// 名字认三种写法：`甲`、`Template:甲`、`template:甲`（大小写与首尾空白都宽松）——
/// 都指 `Template:` 命名空间里的那一页。
pub fn template_page(name: &str) -> Option<TemplatePage> {
    let key = template_page_key(name);
    CURRENT_TEMPLATE_PAGES.with(|cell| {
        cell.borrow()
            .as_ref()
            .and_then(|pages| pages.get(&key).cloned())
    })
}

/// 现在展开到第几层（见 [`deeper`]）
pub fn template_depth() -> usize {
    TEMPLATE_DEPTH.with(|depth| depth.get())
}

/// 深一层地做一件事（用完还原）。
///
/// 用户模板可以嵌用户模板 —— 一个转圈的模板（甲嵌乙、乙嵌甲）会一直展开下去，
/// 所以每展开一层记一笔，超过上限就不展开了（见 `template::expand_user_template`）。
pub fn deeper<T>(run: impl FnOnce() -> T) -> T {
    let previous = TEMPLATE_DEPTH.with(|depth| depth.replace(template_depth() + 1));
    let out = run();
    TEMPLATE_DEPTH.with(|depth| depth.set(previous));
    out
}

/// 页名的归一写法（`Template:甲` 与 `甲` 是同一个键；统一小写）
pub fn template_page_key(name: &str) -> String {
    let trimmed = name.trim();
    let page = match trimmed.split_once(':') {
        Some((prefix, rest))
            if prefix
                .trim()
                .eq_ignore_ascii_case(crate::vault::namespace::TEMPLATE_NAME) =>
        {
            rest.trim()
        }
        _ => trimmed,
    };
    page.to_lowercase()
}

/// 取当前渲染的解析器（供自定义语法使用）
pub fn current_resolver() -> Option<Resolver> {
    CURRENT_RESOLVER.with(|cell| cell.borrow().clone())
}

/// markdown-it 默认是个**空解析器**：CommonMark 语法本身也是插件，必须显式注册。
///
/// 几处刻意为之：
/// - 不注册 `plugins::html`：它会把笔记里的 raw HTML 原样透传，而前端是用
///   `v-html` 注入的，等于把 XSS 入口交给笔记内容。
/// - 不用 syntect（见 Cargo.toml）：它把高亮配色以内联 style 写死，跟不了主题。
///   代码分词改由前端的 highlight.js 负责。
/// - 注册完 cmark 之后再把 Setext 标题规则摘掉，只保留 `#` 形式的 ATX 标题。
/// - `heading_anchors` 不在 `extra::add` 里，要单独注册才会给标题加 id。
static MARKDOWN: LazyLock<MarkdownIt> = LazyLock::new(|| {
    let mut md = MarkdownIt::new();
    markdown_it::plugins::cmark::add(&mut md);

    // 移除 Setext 风格标题（`标题` 下一行写 === 或 ---）。
    // 块级规则在另一个 ruler 里，所以要摘 md.block 而不是 md.remove_rule——
    // 后者只管 CoreRule，写错了会编译通过但完全无效。
    md.block
        .remove_rule::<markdown_it::plugins::cmark::block::lheading::LHeadingScanner>();

    // 移除缩进代码块（四空格开头）。
    //
    // 缩进在这个项目里有了别的用途：**模板块的边界**。两者会互相干扰 ——
    // 一个缩进的模板块很容易先被当成代码块收走。代码块请用围栏写法。
    md.block
        .remove_rule::<markdown_it::plugins::cmark::block::code::CodeScanner>();

    markdown_it::plugins::extra::add(&mut md);

    // 摘掉两条排版规则：它们会连**代码片段**一起改。
    //
    //   `--accent`  →  `–accent`     （不是那个 CSS 变量名了）
    //   "引号"      →  “引号”        （不是那段代码了）
    //
    // 正文里弯引号、长破折号是锦上添花，在代码里却是**改坏内容**。crate 这两条规则不区分
    // 代码片段，所以在技术笔记里只能整个摘掉。（对应测试：code_spans_keep_their_literals）
    md.remove_rule::<markdown_it::plugins::extra::typographer::TypographerRule>();
    md.remove_rule::<markdown_it::plugins::extra::smartquotes::SmartQuotesRule<
        '\u{2018}',
        '\u{2019}',
        '\u{201c}',
        '\u{201d}',
    >>();
    markdown_it::plugins::extra::heading_anchors::add(&mut md, slugify_heading);

    // 自定义语法统一在 syntax/ 里注册
    syntax::register(&mut md);

    md
});

/// 把 markdown 编译成 HTML。不带链接解析，因此内部链接不会被标出红/蓝。
pub fn render(markdown: &str) -> String {
    render_with(markdown, None)
}

/// 再渲染一段 markdown，**沿用当前这一趟的解析器与模板页表**。
///
/// 给 `::html` 里的 `<markdown>` 用：它在渲染途中回调进来，得接着用同一份上下文，
/// 否则那一段里的内部链接会一律变成红链、`::html src=` 也会取不到页。
///
/// 走 [`render_with_pages`] 那套存旧值/还原的逻辑，所以嵌套渲染不会互相踩。
pub fn render_nested(markdown: &str) -> String {
    let resolver = current_resolver();
    let pages = CURRENT_TEMPLATE_PAGES.with(|cell| cell.borrow().clone());
    render_with_pages(markdown, resolver.as_ref(), pages)
}

/// 带链接解析的渲染：`[[目标]]` 会额外带上 `data-key` / `data-title` / `data-missing`。
pub fn render_with(markdown: &str, resolver: Option<&Resolver>) -> String {
    render_with_pages(markdown, resolver, None)
}

/// 带链接解析、并且带上**模板页正文**的渲染（模板的 `src=` 用得上后者）。
pub fn render_with_pages(
    markdown: &str,
    resolver: Option<&Resolver>,
    pages: Option<TemplatePages>,
) -> String {
    // 目标里的空格先补成 `%20`：CommonMark 不认带空格的目标（见下面那个函数）
    let markdown = &encode_spaces_in_targets(markdown);

    // 存下旧值、用完还原：若出现嵌套渲染，也不会互相踩
    let previous = CURRENT_RESOLVER.with(|cell| cell.borrow().clone());
    let previous_pages = CURRENT_TEMPLATE_PAGES.with(|cell| cell.borrow().clone());
    CURRENT_RESOLVER.with(|cell| *cell.borrow_mut() = resolver.cloned());
    CURRENT_TEMPLATE_PAGES.with(|cell| *cell.borrow_mut() = pages);

    let html = MARKDOWN.parse(markdown).render();

    CURRENT_RESOLVER.with(|cell| *cell.borrow_mut() = previous);
    CURRENT_TEMPLATE_PAGES.with(|cell| *cell.borrow_mut() = previous_pages);
    html
}

/// 链接目标里的空格补成 `%20`。
///
/// CommonMark 规定链接目标**不能有空格**：`![屏幕截图 2026.png](屏幕截图 2026.png)`
/// 会被整行当普通文字渲染，而不是图片（要写就得包成 `<…>` 或写成 `%20`）。
/// 可这一页引用的是**文件名**，而文件名里有空格太常见了 —— 截图默认就叫这个。
/// 作者照自己看见的名字写，就应当能用。
///
/// 只动**链接/图片的目标**，而且**跳过代码**：围栏代码块与行内代码里的字一个不改 ——
/// 那里的写法是给人看的例子，改了就成了说谎。
fn encode_spaces_in_targets(markdown: &str) -> String {
    let mut out = String::with_capacity(markdown.len());
    // 当前在不在围栏代码块里（记的是围栏用的记号：` 还是 ~）
    let mut fence: Option<char> = None;

    for line in markdown.split_inclusive('\n') {
        let trimmed = line.trim_start();
        let marker = trimmed.chars().next().filter(|ch| *ch == '`' || *ch == '~');
        if let Some(marker) = marker {
            let run = trimmed.chars().take_while(|ch| *ch == marker).count();
            if run >= 3 {
                match fence {
                    Some(open) if open == marker => fence = None,
                    None => fence = Some(marker),
                    // 另一种记号：不配对，当普通一行
                    _ => {}
                }
                out.push_str(line);
                continue;
            }
        }

        if fence.is_some() {
            out.push_str(line);
        } else {
            out.push_str(&encode_line_targets(line));
        }
    }
    out
}

/// 一行里的目标（`](…)`）补空格编码；行内代码整段跳过
fn encode_line_targets(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::with_capacity(line.len());
    let mut index = 0;

    while index < chars.len() {
        // 行内代码：连反引号一起原样搬走（里面的 `](` 不是目标）
        if chars[index] == '`' {
            let run = chars[index..].iter().take_while(|ch| **ch == '`').count();
            let close = (index + run..chars.len())
                .find(|at| chars[*at..].iter().take_while(|ch| **ch == '`').count() >= run);
            match close {
                Some(close) => {
                    let end = close + chars[close..].iter().take_while(|ch| **ch == '`').count();
                    out.extend(&chars[index..end]);
                    index = end;
                    continue;
                }
                None => {
                    // 没闭合：后面都当普通文字
                    out.extend(&chars[index..]);
                    break;
                }
            }
        }

        // `](` —— 图片与链接的目标都从这里开始
        if chars[index] == ']' && chars.get(index + 1) == Some(&'(') {
            if let Some((end, target)) = encode_target(&chars, index + 2) {
                out.push_str("](");
                out.push_str(&target);
                out.push(')');
                index = end + 1;
                continue;
            }
        }

        out.push(chars[index]);
        index += 1;
    }
    out
}

/// 认出一个目标（`(` 之后到配对的 `)`），把里面的空格编码掉。
///
/// 认不出就返回 `None`（原样留着）：尖括号写法本来就对、跨行的目标不碰、
/// 带引号的多半还挂着标题 —— 那几种交给标准解析器，别自作聪明。
fn encode_target(chars: &[char], start: usize) -> Option<(usize, String)> {
    if chars.get(start) == Some(&'<') {
        return None;
    }

    let mut text = String::new();
    let mut level = 0usize;
    let mut index = start;
    while index < chars.len() {
        match chars[index] {
            '\n' | '\r' => return None,
            '\'' | '"' => return None,
            '(' => level += 1,
            ')' if level == 0 => break,
            ')' => level -= 1,
            _ => {}
        }
        text.push(chars[index]);
        index += 1;
    }

    // 没闭合、没有空格、或者一上来就是空白：都不是我们要处理的
    if index >= chars.len() || !text.contains(' ') || text.starts_with(char::is_whitespace) {
        return None;
    }
    Some((index, text.replace(' ', "%20")))
}

/// 把一段文本包进代码块。围栏要比正文里最长的一串反引号更长，否则会被提前闭合。
pub fn fence_code(text: &str) -> String {
    fence_code_in(text, "")
}

/// 同上，但给围栏标上语言（`language-css` 之类），供前端分词。
pub fn fence_code_in(text: &str, language: &str) -> String {
    let mut longest = 0usize;
    let mut run = 0usize;
    for ch in text.chars() {
        if ch == '`' {
            run += 1;
            longest = longest.max(run);
        } else {
            run = 0;
        }
    }
    let fence = "`".repeat((longest + 1).max(3));
    format!("{fence}{language}\n{}\n{fence}\n", text.trim_end())
}

#[cfg(test)]
mod target_space_tests {
    use super::{encode_spaces_in_targets, render};

    /// 目标里的空格补成 `%20`：作者照文件名写，就该渲染成图片
    #[test]
    fn spaces_in_targets_become_percent_twenty() {
        let html = render("![屏幕截图 20260925 144347.png](屏幕截图 20260925 144347.png)\n");
        assert!(html.contains("<img"), "{html}");
        assert!(
            html.contains("%E5%B1%8F%E5%B9%95%E6%88%AA%E5%9B%BE%2020260925%20144347.png"),
            "{html}"
        );
        assert!(
            html.contains("alt=\"屏幕截图 20260925 144347.png\""),
            "{html}"
        );

        // 普通链接也一样
        let link = render("[看看](我的 图.png)\n");
        assert!(
            link.contains("<a href=\"%E6%88%91%E7%9A%84%20%E5%9B%BE.png\""),
            "{link}"
        );
    }

    /// 代码里的写法**一个不改**：那是给人看的例子
    #[test]
    fn code_keeps_its_literals() {
        let fenced = render("```\n![a](b c.png)\n```\n");
        assert!(fenced.contains("](b c.png)"), "{fenced}");
        assert!(!fenced.contains("%20"), "{fenced}");

        let inline = render("写法是 `![a](b c.png)` 这样\n");
        assert!(inline.contains("](b c.png)"), "{inline}");
        assert!(!inline.contains("%20"), "{inline}");
    }

    /// 本来就对、或者不能确定意图的写法：原样留着
    #[test]
    fn other_targets_are_left_alone() {
        // 尖括号写法标准解析器自己会处理
        assert_eq!(
            encode_spaces_in_targets("![a](<b c.png>)"),
            "![a](<b c.png>)"
        );
        // 没有空格的目标不必动
        assert_eq!(encode_spaces_in_targets("![a](b.png)"), "![a](b.png)");
        // 带引号的多半挂着标题：不猜
        assert_eq!(
            encode_spaces_in_targets("![a](b.png \"标题 字\")"),
            "![a](b.png \"标题 字\")"
        );
        // 跨行的目标不存在
        assert_eq!(encode_spaces_in_targets("![a](b\nc.png)"), "![a](b\nc.png)");
        // 括号配对的不受影响（文件名里有括号）
        assert_eq!(
            encode_spaces_in_targets("![a](图 (1).png)"),
            "![a](图%20(1).png)"
        );
    }
}

#[cfg(test)]
mod tests {
    /// 围栏要比正文里最长的一串反引号更长，否则会被提前闭合
    #[test]
    fn fence_is_longer_than_the_longest_run() {
        assert!(super::fence_code("普通正文").starts_with("```\n"));
        assert!(super::fence_code("里面有 ``` 三个").starts_with("````\n"));
    }
    use super::*;

    #[test]
    fn slugify_keeps_cjk_and_folds_the_rest() {
        assert_eq!(slugify_heading("文本与行内元素"), "文本与行内元素");
        assert_eq!(slugify_heading("Hello World"), "hello-world");
        assert_eq!(slugify_heading("H1 Test"), "h1-test");
        // 连续的非字母数字（空格、斜杠）只归并成一个 '-'，首尾不留 '-'
        assert_eq!(slugify_heading("  Code / Notes  "), "code-notes");
        assert_eq!(slugify_heading("!!!"), "");
    }

    #[test]
    fn headings_get_anchor_ids() {
        let html = render("## 文本与行内元素\n");
        assert!(html.contains(r#"id="文本与行内元素""#), "{html}");
    }

    /// 锁住一个前端必须配合的行为：href 会被 URL 编码，而 id 是未编码的原文。
    /// 所以前端在 getElementById 之前必须先 decodeURIComponent。
    #[test]
    fn anchor_href_is_url_encoded() {
        let html = render("[跳转](#表格)\n\n## 表格\n");
        assert!(html.contains(r#"id="表格""#), "标题缺 id: {html}");
        // 用 r##"..."## 而不是 r#"..."#：内容里出现了 "# 序列，
        // 那正好是后者的结束分隔符，会把字符串提前截断。
        assert!(html.contains(r##"href="#%E8%A1%A8%E6%A0%BC""##), "{html}");
    }

    /// **重名的标题拿到的是同一个 `id`** —— 这条钉的是**现状**，不是愿望。
    ///
    /// [`slugify_heading`] 是纯函数，而 `heading_anchors` 直接拿它的返回值当 `id`，
    /// 所以三个 `## 表格` 就是三个 `id="表格"`（实测如此，2026-10 记）。
    ///
    /// 记下来是因为前端**已经踩到并处理了**这一条：`components/note/OutlinePanel.vue`
    /// 的 `v-for :key` 用的是 `key` 而不是 `id`，`dom/outline.ts` 按「第几个同名的」
    /// 定位元素。哪天这一层真去给重名错开（`-1`、`-2`），那边要跟着改 ——
    /// 而错开与否对正文里的 `[文字](#表格)` 是有影响的（它指向第一个）。
    #[test]
    fn repeated_headings_share_one_anchor_id() {
        let html = render("## 表格\n\n## 表格\n\n## 表格\n");
        assert_eq!(
            html.matches(r##"id="表格""##).count(),
            3,
            "三个同名标题，三个一样的 id：{html}"
        );
        // 没有悄悄错开成 -1 / -2（那样前端那边的"第几个同名的"就白做了）
        assert!(!html.contains("表格-1"), "重名并没有被错开：{html}");
    }

    /// 代码片段里的字面量不许被排版规则改写。
    ///
    /// 排版规则（typographer / smartquotes）会把 `--` 变成 `–`、把直引号变成弯引号 ——
    /// 在正文里是锦上添花，在代码片段里则是**改坏了内容**：`--accent` 变成 `–accent`
    /// 就不再是那个 CSS 变量名了。
    #[test]
    fn code_spans_keep_their_literals() {
        // 用原始字符串写，免得反斜杠在两层转义里走样
        let source = r#"`--accent` 与 `a -- b` 与 "引号""#;
        let html = render(source);
        assert!(html.contains("--accent"), "破折号被改写了：{html}");
        assert!(html.contains("a -- b"), "破折号被改写了：{html}");
        // 直引号在 HTML 里会转义成 &quot;，那是应有的转义；要拦的是被换成弯引号
        assert!(!html.contains('\u{201c}'), "引号被换成弯引号了：{html}");
        assert!(
            html.contains("&quot;引号&quot;"),
            "直引号应当原样保留：{html}"
        );
    }

    /// 缩进代码块语法已关闭：四空格开头不再是代码块。
    ///
    /// 关掉它是因为缩进现在专用于**模板块的边界**，两者会互相干扰。
    #[test]
    fn indented_code_blocks_are_disabled() {
        let html = render("    四个空格开头\n");
        assert!(!html.contains("<pre"), "不该再被当成代码块：{html}");
        assert!(html.contains("四个空格开头"), "{html}");
    }

    /// raw HTML 必须被转义：前端是用 v-html 注入的，这里等于 XSS 边界
    #[test]
    fn raw_html_is_escaped() {
        let html = render("<script>alert(1)</script>\n");
        assert!(!html.contains("<script"), "{html}");
        assert!(html.contains("&lt;script"), "{html}");
    }

    #[test]
    fn dangerous_link_protocols_are_rejected() {
        let html = render("[x](javascript:alert(1))\n");
        assert!(!html.contains(r#"href="javascript:"#), "{html}");
    }

    #[test]
    fn fenced_code_keeps_language_class() {
        let html = render("```rust\nfn main() {}\n```\n");
        assert!(html.contains(r#"<code class="language-rust">"#), "{html}");
    }

    #[test]
    fn setext_headings_are_removed() {
        let html = render("标题\n===\n");
        assert!(!html.contains("<h1"), "Setext 标题应当已被移除: {html}");
        assert!(!html.contains("<h2"), "Setext 标题应当已被移除: {html}");

        // 下划线是 `---` 时更要确认：它本身就是 thematic break，
        // 移除 Setext 后会退化成「段落 + 一条分隔线」，而不是标题。
        let html = render("标题\n---\n");
        assert!(!html.contains("<h1"), "{html}");
        assert!(!html.contains("<h2"), "{html}");
        assert!(html.contains("<hr"), "--- 应当退化成分隔线: {html}");
    }

    #[test]
    fn atx_headings_still_work() {
        let html = render("# 一级\n\n## 二级\n");
        assert!(html.contains("<h1"), "{html}");
        assert!(html.contains("<h2"), "{html}");
    }

    /// 渲染状态必须是「每次调用各自一份」：并行渲染时不能互相干扰。
    ///
    /// 这条有来历——最初把解析器塞进解析器的全局 `ext` 集合，并行跑测试立刻出现
    /// 「一个线程的清理打断了另一个线程的渲染」，于是改成线程局部变量。
    #[test]
    fn resolver_does_not_leak_across_threads() {
        use std::collections::HashSet;
        use std::sync::Arc;

        let handles: Vec<_> = (0..4)
            .map(|index| {
                std::thread::spawn(move || {
                    let mut keys = HashSet::new();
                    keys.insert(format!("0:条目{index}"));
                    let resolver = crate::vault::target::Resolver::new(
                        Arc::new(crate::vault::namespace::NamespaceTable::builtin()),
                        Arc::new(crate::vault::target::PageIndex::of_keys(keys)),
                        None,
                    );
                    let html = render_with(
                        &format!("[[条目{index}]] 与 [[别的条目]]\n"),
                        Some(&resolver),
                    );
                    assert!(
                        html.contains(&format!(r#"data-key="0:条目{index}""#)),
                        "{html}"
                    );
                    assert!(html.contains(r#"data-missing="true""#), "{html}");
                })
            })
            .collect();

        for handle in handles {
            handle.join().unwrap();
        }

        // 渲染结束后不留残留：不带解析器的渲染不会标红/蓝
        let html = render("[[条目0]]\n");
        assert!(!html.contains("data-key"), "{html}");
    }
}
