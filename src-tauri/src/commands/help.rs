//! 帮助页：随程序发布的那几页（只读）。

use crate::features;
use crate::open_database;

/// 帮助页清单（页面名与标题；正文也一并给出，页数不多）
#[tauri::command]
pub fn help_pages() -> Result<Vec<features::help::HelpPage>, String> {
    let (_, database) = open_database()?;
    Ok(features::help::pages(&database))
}

/// 读一页帮助（含渲染好的 HTML）
#[tauri::command]
pub fn read_help(page: String) -> Result<features::help::HelpPage, String> {
    let (_, database) = open_database()?;
    features::help::find(&database, &page).ok_or_else(|| format!("没有这页帮助：{page}"))
}
