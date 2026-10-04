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

//! 笔记：地址解析、读与写、草稿、历史、回退、口令。

use crate::open_database;
use crate::storage::codec::Policy;
use crate::storage::session;
use crate::vault::address::{self, ParsedAddress};
use crate::vault::notes::{Draft, Note, NoteSummary, Reading, RevisionSummary};
use crate::vault::resolve::{self, ExportFormat, ResolvedAddress};
use tauri::AppHandle;

/// 解析地址栏那一行（只做语法）。空输入不是地址，返回 `None`；
/// 语法有问题时，错误里是一句给人看的话。
#[tauri::command]
pub fn parse_address(input: String) -> Result<Option<ParsedAddress>, String> {
    let (_, database) = open_database()?;
    address::parse(&input, &database.namespaces())
}

/// 解析并落到仓库上：这一页在不在、是不是特殊页（`special:random` 会挑一篇落下去）
#[tauri::command]
pub fn resolve_address(input: String) -> Result<Option<ResolvedAddress>, String> {
    let (_, database) = open_database()?;
    database.resolve_address(&input)
}

#[tauri::command]
pub fn read_note(title: String, reference: Option<String>) -> Result<Reading, String> {
    let (_, database) = open_database()?;
    database.read_note(&title, reference.as_deref())
}

/// 导出某一版到用户选的位置。
///
/// `format` 是 `markdown` / `html` / `pdf`（大小写宽松）；不给时按 `markdown` ——
/// 那是这个命令原本唯一做的事，老界面不传也不该坏。
///
/// 路径由系统保存对话框给出（手机上没有那一步，落到下载目录，见
/// `platform::saving::resolve`）。
#[tauri::command]
pub fn export_note(
    app: AppHandle,
    title: String,
    reference: Option<String>,
    target: Option<String>,
    format: Option<String>,
) -> Result<String, String> {
    let (_, database) = open_database()?;
    let format = match format.as_deref() {
        None => ExportFormat::Markdown,
        Some(text) => ExportFormat::parse(text)?,
    };

    // PDF 不在 Rust 里生成字节（理由见 `platform::print` 的抬头）：界面在一个隐藏
    // iframe 里载入打印版 HTML、调浏览器自己的打印，那里才有"另存为 PDF"。
    // 所以这一支**不需要路径**，也就不到 `saving::resolve` 那一步。
    if format == ExportFormat::Pdf {
        return print_note_html(&database, &title, reference.as_deref());
    }

    // 没给路径（手机上）就落进下载目录；给的是 `content://…` 也当没给
    let target = crate::platform::saving::resolve(
        &app,
        target,
        &format.extensionless_name(&note_file_stem(&title)),
    )?;

    match format {
        ExportFormat::Markdown => {
            database.export_note(&title, reference.as_deref(), &target)?;
        }
        ExportFormat::Html => {
            database.export_note_html(&title, reference.as_deref(), &target)?;
        }
        // 上面已经返回了
        ExportFormat::Pdf => unreachable!("PDF 那一支在上面就走了"),
    }
    Ok(target.to_string_lossy().to_string())
}

/// 为「导出 PDF」准备打印用的 HTML，交给界面去打印。
///
/// 单独一条命令而不是把三件事塞进 `export_note` 的返回值：这一支**没有路径**
/// （路径由浏览器的保存对话框决定），所以它的返回值与"导出了某个文件"不是一回事。
/// 让界面分两次调用，才不会把一段 HTML 当成"已保存的路径"显示出来。
#[tauri::command]
pub fn note_print_html(
    title: String,
    reference: Option<String>,
) -> Result<String, String> {
    let (_, database) = open_database()?;
    print_note_html(&database, &title, reference.as_deref())
}

/// 渲染打印版 HTML（真身，测试与两个入口都走它）。
///
/// 用的是 `read_note` 里**已经渲染好**的那份 `note.html`，不自己再渲染一遍 ——
/// 渲染只有一处（见 `vault::notes::read` 顶上那段），所以"打印出来的"
/// 与"阅读页看到的"必然一致，而不会出现两条渲染路径悄悄分家。
fn print_note_html(
    database: &crate::vault::database::Database,
    title: &str,
    reference: Option<&str>,
) -> Result<String, String> {
    let crate::vault::notes::Reading::Ready { note } = database.read_note(title, reference)?
    else {
        return Err("这一版是加密的：先解锁，再导出".to_string());
    };
    Ok(crate::platform::print::wrap_for_print(&note.title, &note.html))
}

/// 导出时的默认文件名（**不含扩展名**）：标题里的斜杠（子页面）与文件系统不认的
/// 字符都换成 `-`。扩展名由 [`ExportFormat`] 补。
///
/// 与界面那边 `dom/file-save.ts::noteFileName` 是同一条规矩：桌面走系统对话框时
/// 名字由界面给，手机上进下载目录时由这里给。
fn note_file_stem(title: &str) -> String {
    let safe: String = title
        .chars()
        .map(|ch| match ch {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            other => other,
        })
        .collect();
    let safe = safe.trim();
    if safe.is_empty() { "笔记".to_string() } else { safe.to_string() }
}

#[tauri::command]
pub fn create_note(title: String) -> Result<String, String> {
    let (_, database) = open_database()?;
    database.create(&title)
}

/// 提交一版。`protection` 显式给出就是**换保护**；不给就照这篇当前的保护。
#[tauri::command]
pub fn commit_note(
    title: String,
    markdown: String,
    summary: Option<String>,
    protection: Option<Policy>,
    passphrase: Option<String>,
) -> Result<Note, String> {
    let (_, database) = open_database()?;
    database.commit_with(&title, &markdown, summary, protection, passphrase)
}

#[tauri::command]
pub fn load_draft(title: String) -> Result<Option<Draft>, String> {
    let (_, database) = open_database()?;
    database.load_draft(&title)
}

#[tauri::command]
pub fn save_draft(title: String, markdown: String) -> Result<(), String> {
    let (_, database) = open_database()?;
    database.save_draft(&title, &markdown)
}

#[tauri::command]
pub fn discard_draft(title: String) -> Result<bool, String> {
    let (_, database) = open_database()?;
    database.discard_draft(&title)
}

#[tauri::command]
pub fn list_revisions(title: String) -> Result<Vec<RevisionSummary>, String> {
    let (_, database) = open_database()?;
    database.revisions_of(&title)
}

/// 回滚到某一版（永远是**新增一个提交**）。
/// `reference` 是地址里的版本 token；`copy` 为真时复制那一版的封装（不解锁），
/// 为假时解锁那一版重写 —— 重写可以顺带指定**新的保护**（`protection`，
/// 不给就照这篇当前的保护）与它的口令（`passphrase`，只在要套对称层时用得上）。
/// 返回新版本号。
#[tauri::command]
pub fn rollback_note(
    title: String,
    reference: String,
    summary: Option<String>,
    copy: bool,
    protection: Option<Policy>,
    passphrase: Option<String>,
) -> Result<u64, String> {
    let (_, database) = open_database()?;
    database.rollback_note(&title, &reference, summary, copy, protection, passphrase)
}

/// 某一版落盘封装的细节（签名验得怎么样、加密到谁、口令这次会话里有没有）
#[tauri::command]
pub fn protection_report(
    title: String,
    reference: Option<String>,
) -> Result<resolve::ProtectionReport, String> {
    let (_, database) = open_database()?;
    database.protection_report(&title, reference.as_deref())
}

#[tauri::command]
pub fn delete_note(title: String) -> Result<(), String> {
    let (_, database) = open_database()?;
    database.delete(&title)
}

#[tauri::command]
pub fn list_notes() -> Result<Vec<NoteSummary>, String> {
    let (_, database) = open_database()?;
    database.list()
}

/// 编辑器预览：把 markdown 渲染成 HTML，与阅读页**同一个渲染器**
#[tauri::command]
pub fn render_markdown(markdown: String, title: String) -> Result<String, String> {
    let (_, database) = open_database()?;
    database.render_html(&markdown, &title)
}

/// 给某一版解锁（`reference` 是版本 token，`None` = 最新版）
#[tauri::command]
pub fn unlock(title: String, reference: Option<String>, passphrase: String) -> Result<(), String> {
    let (_, database) = open_database()?;
    database.unlock(&title, reference.as_deref(), &passphrase)
}

/// 忘掉这次会话里所有口令
#[tauri::command]
pub fn lock() {
    session::forget_all();
}

/// 这一版的口令在不在本次会话里（界面上的"口令已暂存"）
#[tauri::command]
pub fn passphrase_stored(title: String, reference: Option<String>) -> Result<bool, String> {
    let (_, database) = open_database()?;
    database.passphrase_stored(&title, reference.as_deref())
}

/// 忘掉这一篇在这次会话里存过的口令（它的每一版）
#[tauri::command]
pub fn forget_passphrase(title: String) -> Result<(), String> {
    let (_, database) = open_database()?;
    let id = database.locate(&title)?;
    session::forget_note(&id);
    Ok(())
}
