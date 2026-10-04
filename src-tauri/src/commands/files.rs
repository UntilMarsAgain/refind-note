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

//! 文件页面：上传、更新、改名、删除、另存为、交给系统打开。

use crate::features::files;
use crate::open_database;
use crate::platform::{decode_percent, staging};
use crate::storage::codec::Policy;
use tauri::AppHandle;

/// 附件清单（新的在前）
#[tauri::command]
pub fn list_files() -> Result<Vec<files::FileEntry>, String> {
    let (_, database) = open_database()?;
    database.list_files()
}

/// 从系统文件对话框选的路径收一个文件（字节由后端自己读，不走 IPC）
#[tauri::command]
pub fn upload_file(
    path: String,
    protection: Option<Policy>,
    passphrase: Option<String>,
) -> Result<files::Uploaded, String> {
    let (_, database) = open_database()?;
    let source = std::path::PathBuf::from(&path);
    let bytes = std::fs::read(&source).map_err(|error| format!("读不到这个文件：{error}"))?;
    let name = source
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "未命名".to_string());
    let mime = files::mime_of(&name);
    database.add_file(&name, &bytes, mime, protection, passphrase)
}

/// 给一个**已经存在的文件页面**传新版（更新）
#[tauri::command]
pub fn update_file(
    title: String,
    path: String,
    protection: Option<Policy>,
    passphrase: Option<String>,
) -> Result<files::Uploaded, String> {
    let (_, database) = open_database()?;
    let source = std::path::PathBuf::from(&path);
    let bytes = std::fs::read(&source).map_err(|error| format!("读不到这个文件：{error}"))?;
    // 名字沿用页面名：更新不该顺手改名
    let name = database.parse_title(&title)?.page;
    let mime = files::mime_of(&name);
    database.add_file(&name, &bytes, mime, protection, passphrase)
}

/// 粘贴进来的字节直接走二进制通道（名字放在头里）。
///
/// 剪贴板里的文件没有路径可读，只能把字节递过来；用原始 IPC 体而不是 base64，
/// 大图才不会在编码上再翻一倍。
#[tauri::command]
pub fn upload_bytes(request: tauri::ipc::Request<'_>) -> Result<files::Uploaded, String> {
    let (_, database) = open_database()?;
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err("粘贴上传要走二进制通道，但没收到字节".to_string());
    };
    // 走二进制通道时，请求体整个是**字节**，别的参数塞不进去 —— 只能放头里（见下）
    let name = header_arg(&request, "x-file-name")
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "粘贴的文件".to_string());
    let mime = files::mime_of(&name);
    database.add_file(
        &name,
        bytes,
        mime,
        protection_arg(&request),
        passphrase_arg(&request),
    )
}

/// 请求头里的一个参数（值按百分号编码过：头里只放得下 ASCII）。
///
/// 只在**原始字节**那条通道上用：那条通道的请求体是文件内容本身，
/// 命令的其余参数没有别的地方可放（与 `x-file-name` 同一个道理）。
fn header_arg(request: &tauri::ipc::Request<'_>, name: &str) -> Option<String> {
    request
        .headers()
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(decode_percent)
}

/// 请求头里的"怎么存"（JSON）；没给就是照这一页当前的
fn protection_arg(request: &tauri::ipc::Request<'_>) -> Option<Policy> {
    serde_json::from_str(&header_arg(request, "x-protection")?).ok()
}

/// 请求头里的口令
fn passphrase_arg(request: &tauri::ipc::Request<'_>) -> Option<String> {
    header_arg(request, "x-passphrase").filter(|value| !value.is_empty())
}

/// 一个文件的现状（怎么存的、现在读不读得动）—— 界面据此决定"直接显示还是先解锁"。
/// `reference` 给版本 token 就看那一版
#[tauri::command]
pub fn file_info(key: String, reference: Option<String>) -> Result<files::FileInfo, String> {
    let (_, database) = open_database()?;
    database.file_info(&key, reference.as_deref())
}

/// 用**系统默认应用**打开这一版。
///
/// 仓库里存的是字节，没有一个"能在文件管理器里双击"的路径 —— 所以先把它落到
/// 一个临时文件上，再把那个路径交给系统的打开方式。落在临时目录里是**刻意**的：
/// 它是给外部程序看的副本，用完由系统回收，不进仓库、也不该被当成原件的家。
/// 加密存的内容到了这一步已经是明文，所以落盘时把权限收紧（见 `stage_file`）。
#[tauri::command]
pub fn open_file(
    app: tauri::AppHandle,
    title: String,
    reference: Option<String>,
) -> Result<String, String> {
    let (_, database) = open_database()?;
    let (bytes, _mime) = database.read_file(&title, reference.as_deref())?;
    let page = database.parse_title(&title)?.page;
    let path = staging::stage_file(&page, &bytes)?;
    let shown = path.to_string_lossy().to_string();
    tauri_plugin_opener::OpenerExt::opener(&app)
        .open_path(shown.clone(), None::<String>)
        .map_err(|error| format!("交给系统打开失败：{error}"))?;
    // 把落点告诉界面：临时副本在哪儿，值得让人知道（尤其加密的那些）
    Ok(shown)
}

/// 改文件的名字（页面名跟着改）
#[tauri::command]
pub fn rename_file(title: String, name: String) -> Result<files::FileEntry, String> {
    let (_, database) = open_database()?;
    database.rename_file(&title, &name)
}

/// 删一个文件（进回收站）
#[tauri::command]
pub fn delete_file(title: String) -> Result<(), String> {
    let (_, database) = open_database()?;
    database.delete_file(&title)
}

/// 另存为：把某一版复制到用户选的位置
#[tauri::command]
pub fn export_file(
    app: AppHandle,
    title: String,
    target: Option<String>,
) -> Result<String, String> {
    let (_, database) = open_database()?;
    // 没给路径（手机上）就按页面名落进下载目录
    let name = title.trim().split(':').next_back().unwrap_or(&title).trim();
    let target = crate::platform::saving::resolve(&app, target, name)?;
    database.export_file(&title, &target)?;
    Ok(target.to_string_lossy().to_string())
}
