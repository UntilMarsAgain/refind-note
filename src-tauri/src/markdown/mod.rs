//! Markdown 解析：插件注册、自定义语法挂载，以及 markdown → HTML 的入口。
//!
//! 解析器只构建一次（见 [`MARKDOWN`]）；解析本身取 `&self`，可复用。

pub mod syntax;

use markdown_it::MarkdownIt;
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

    markdown_it::plugins::extra::add(&mut md);
    markdown_it::plugins::extra::heading_anchors::add(&mut md, slugify_heading);

    // 自定义语法统一在 syntax/ 里注册
    syntax::register(&mut md);

    md
});

/// 把 markdown 编译成 HTML。
pub fn render(markdown: &str) -> String {
    MARKDOWN.parse(markdown).render()
}

#[cfg(test)]
mod tests {
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
}
