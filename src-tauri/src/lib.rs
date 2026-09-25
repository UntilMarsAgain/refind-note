use markdown_it::MarkdownIt;
use serde::Serialize;
use std::sync::LazyLock;

/// 先用编译期嵌入的示例内容顶上。
/// 之后改成按标题读取真实笔记时，只需要替换这里取 markdown 的方式。
const SAMPLE_TITLE: &str = "渲染链路验证";
const SAMPLE_MARKDOWN: &str = include_str!("../sample-note.md");

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
/// 注册只需一次，而解析是 `&self`，所以用 LazyLock 复用同一个实例。
///
/// 三处刻意为之：
/// - 不注册 `plugins::html`：它会把笔记里的 raw HTML 原样透传，而前端是用
///   `v-html` 注入的，等于把 XSS 入口交给笔记内容。
/// - 不用 syntect（见 Cargo.toml）：它把高亮配色以内联 style 写死，跟不了主题。
///   代码分词改由前端的 highlight.js 负责。
/// - `heading_anchors` 不在 `extra::add` 里，要单独注册，它才会给标题加 id。
static MARKDOWN: LazyLock<MarkdownIt> = LazyLock::new(|| {
    let mut md = MarkdownIt::new();
    markdown_it::plugins::cmark::add(&mut md);
    markdown_it::plugins::extra::add(&mut md);
    markdown_it::plugins::extra::heading_anchors::add(&mut md, slugify_heading);
    md
});

/// 前端渲染一篇笔记所需的全部数据。
/// HTML 在 Rust 端就编译好，前端不再参与解析。
#[derive(Serialize)]
pub struct Note {
    title: String,
    html: String,
}

#[tauri::command]
fn load_note() -> Note {
    Note {
        title: SAMPLE_TITLE.to_owned(),
        html: MARKDOWN.parse(SAMPLE_MARKDOWN).render(),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .invoke_handler(tauri::generate_handler![load_note])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
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
        let html = MARKDOWN.parse("## 文本与行内元素\n").render();
        assert!(html.contains(r#"id="文本与行内元素""#), "{html}");
    }

    /// 锁住一个前端必须配合的行为：href 会被 URL 编码，而 id 是未编码的原文。
    /// 所以前端在 getElementById 之前必须先 decodeURIComponent。
    /// 这条断言的具体形式由实测确定（见下方失败信息里打印的真实 HTML）。
    #[test]
    fn anchor_href_is_url_encoded() {
        let html = MARKDOWN.parse("[跳转](#表格)\n\n## 表格\n").render();
        assert!(html.contains(r#"id="表格""#), "标题缺 id: {html}");
        // 用 r##"..."## 而不是 r#"..."#：内容里出现了 "# 序列，
        // 那正好是后者的结束分隔符，会把字符串提前截断。
        assert!(html.contains(r##"href="#%E8%A1%A8%E6%A0%BC""##), "{html}");
    }

    /// raw HTML 必须被转义：前端是用 v-html 注入的，这里等于 XSS 边界
    #[test]
    fn raw_html_is_escaped() {
        let html = MARKDOWN.parse("<script>alert(1)</script>\n").render();
        assert!(!html.contains("<script"), "{html}");
        assert!(html.contains("&lt;script"), "{html}");
    }

    /// markdown-it 自带的链接协议校验
    #[test]
    fn dangerous_link_protocols_are_rejected() {
        let html = MARKDOWN.parse("[x](javascript:alert(1))\n").render();
        assert!(!html.contains(r#"href="javascript:"#), "{html}");
    }

    /// 代码块保留语言类名，highlight.js 靠它选词法
    #[test]
    fn fenced_code_keeps_language_class() {
        let html = MARKDOWN.parse("```rust\nfn main() {}\n```\n").render();
        assert!(html.contains(r#"<code class="language-rust">"#), "{html}");
    }

    #[test]
    fn sample_note_renders() {
        let note = load_note();
        assert_eq!(note.title, SAMPLE_TITLE);
        assert!(note.html.contains("<table>"), "表格插件应当生效");
        assert!(note.html.contains("<h1"), "示例里留了 H1 用于对比字号");
    }
}
