//! Tauri 端的入口与命令。
//!
//! Markdown 的一切都在 [`markdown`] 模块里，自定义语法在
//! [`markdown::syntax`]；这里只负责取数与对外暴露命令。

mod markdown;

use serde::Serialize;

/// 先用编译期嵌入的示例内容顶上。
/// 之后改成按标题读取真实笔记时，只需要替换这里取 markdown 的方式。
const SAMPLE_TITLE: &str = "渲染链路验证";
const SAMPLE_MARKDOWN: &str = include_str!("../sample-note.md");

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
        html: markdown::render(SAMPLE_MARKDOWN),
    }
}

/// 内部链接的跳转还没有真实笔记来源，先只把调用打到后端命令行
/// （也就是跑 `pnpm tauri dev` 的那个终端）。
/// 之后接上「按标题读取笔记」时，把这里换成真正的跳转即可。
#[tauri::command]
fn open_wikilink(doc: String) {
    println!("[wikilink] 待接入：打开《{doc}》");
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .invoke_handler(tauri::generate_handler![load_note, open_wikilink])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_note_renders() {
        let note = load_note();
        assert_eq!(note.title, SAMPLE_TITLE);
        assert!(note.html.contains("<table>"), "表格插件应当生效");
        assert!(note.html.contains("<h1"), "示例里留了 H1 用于对比字号");
        assert!(note.html.contains("wikilink"), "示例里留了内部链接");
    }
}
