use markdown_it::MarkdownIt;
use serde::Serialize;
use std::sync::LazyLock;

/// 先用编译期嵌入的示例内容顶上。
/// 之后改成按标题读取真实笔记时，只需要替换这里取 markdown 的方式。
const SAMPLE_TITLE: &str = "渲染链路验证";
const SAMPLE_MARKDOWN: &str = include_str!("../sample-note.md");

/// markdown-it 默认是个**空解析器**：CommonMark 语法本身也是插件，必须显式注册。
/// 注册只需一次，而解析是 `&self`，所以用 LazyLock 复用同一个实例。
///
/// 两处刻意为之：
/// - 不注册 `plugins::html`：它会把笔记里的 raw HTML 原样透传，而前端是用
///   `v-html` 注入的，等于把 XSS 入口交给笔记内容。
/// - 不用 syntect（见 Cargo.toml）：它把高亮配色以内联 style 写死，跟不了主题。
static MARKDOWN: LazyLock<MarkdownIt> = LazyLock::new(|| {
    let mut md = MarkdownIt::new();
    markdown_it::plugins::cmark::add(&mut md);
    markdown_it::plugins::extra::add(&mut md);
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
        .invoke_handler(tauri::generate_handler![load_note])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
