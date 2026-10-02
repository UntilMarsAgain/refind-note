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
use crate::vault::resolve::{self, ResolvedAddress};
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

/// 读一篇笔记（`reference` 是地址里的版本 token，`None` = 最新版）。
/// 读不到不是错误：上了锁会明说。
/// 导出某一版的 markdown 原文到用户选的位置（路径由系统保存对话框给出）
#[tauri::command]
pub fn export_note(
    app: AppHandle,
    title: String,
    reference: Option<String>,
    target: Option<String>,
) -> Result<String, String> {
    let (_, database) = open_database()?;
    // 没给路径（手机上）就落进下载目录；给的是 `content://…` 也当没给
    let target = crate::platform::saving::resolve(&app, target, &note_file_name(&title))?;
    database.export_note(&title, reference.as_deref(), &target)?;
    Ok(target.to_string_lossy().to_string())
}

/// 导出时的默认文件名：标题里的斜杠（子页面）与文件系统不认的字符都换成 `-`。
///
/// 与界面那边 `dom/file-save.ts::noteFileName` 是同一条规矩：桌面走系统对话框时
/// 名字由界面给，手机上进下载目录时由这里给。
fn note_file_name(title: &str) -> String {
    let safe: String = title
        .chars()
        .map(|ch| match ch {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '-',
            other => other,
        })
        .collect();
    let safe = safe.trim();
    format!("{}.md", if safe.is_empty() { "笔记" } else { safe })
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
