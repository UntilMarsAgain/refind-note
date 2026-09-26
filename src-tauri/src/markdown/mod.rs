//! Markdown 解析：插件注册、自定义语法挂载，以及 markdown → HTML 的入口。
//!
//! 解析器只构建一次（见 [`MARKDOWN`]）；解析本身取 `&self`，可复用。

pub mod syntax;

use crate::title::LinkResolver;
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
    static CURRENT_RESOLVER: RefCell<Option<LinkResolver>> = const { RefCell::new(None) };
}

/// 取当前渲染的解析器（供自定义语法使用）
pub fn current_resolver() -> Option<LinkResolver> {
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
    md.remove_rule::<
        markdown_it::plugins::extra::smartquotes::SmartQuotesRule<'\u{2018}', '\u{2019}', '\u{201c}', '\u{201d}'>,
    >();
    markdown_it::plugins::extra::heading_anchors::add(&mut md, slugify_heading);

    // 自定义语法统一在 syntax/ 里注册
    syntax::register(&mut md);

    md
});

/// 把 markdown 编译成 HTML。不带链接解析，因此内部链接不会被标出红/蓝。
///
/// 应用里读笔记走的是 [`render_with`]（要标红/蓝链），这条留给测试与不需要链接
/// 解析的场合，所以在非测试构建里会被报告为「未使用」。
#[allow(dead_code)]
pub fn render(markdown: &str) -> String {
    render_with(markdown, None)
}

/// 带链接解析的渲染：`[[目标]]` 会额外带上 `data-key` / `data-title` / `data-missing`。
pub fn render_with(markdown: &str, resolver: Option<&LinkResolver>) -> String {
    // 存下旧值、用完还原：万一日后出现嵌套渲染，也不会互相踩
    let previous = CURRENT_RESOLVER.with(|cell| cell.borrow().clone());
    CURRENT_RESOLVER.with(|cell| *cell.borrow_mut() = resolver.cloned());

    let html = MARKDOWN.parse(markdown).render();

    CURRENT_RESOLVER.with(|cell| *cell.borrow_mut() = previous);
    html
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
        assert!(html.contains("&quot;引号&quot;"), "直引号应当原样保留：{html}");
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
        use crate::title::NamespaceTable;
        use std::collections::HashSet;
        use std::sync::Arc;

        let handles: Vec<_> = (0..4)
            .map(|index| {
                std::thread::spawn(move || {
                    let mut keys = HashSet::new();
                    keys.insert(format!("0:条目{index}"));
                    let resolver = LinkResolver::new(
                        Arc::new(NamespaceTable::builtin()),
                        Arc::new(keys),
                        true,
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
