mod address;
pub mod codec;
pub mod database;
pub mod markdown;
pub mod notes;
pub mod resolve;
pub mod session;
pub mod settings;
pub mod store;
pub mod title;
pub mod workspace;

use serde::Serialize;

use address::ParsedAddress;
use codec::Policy;
use database::{Config, Database, Meta};
use notes::{Draft, Note, NoteSummary, Reading, RevisionSummary};
use resolve::ResolvedAddress;
use settings::Preferences;
use workspace::Workspace;

/// 工作目录与数据库的概览：启动时读一次，界面据此显示"东西存在哪、什么状态"
#[derive(Serialize)]
struct WorkspaceInfo {
    /// 工作目录（给人看的位置）
    root: String,
    /// 数据库目录
    database_root: String,
    meta: Meta,
    preferences: Preferences,
    /// 仓库默认的保护策略（新笔记从它出发）
    protection: Policy,
    /// 这台计算机上有没有 gpg（没有时签名 / 加密不可用）
    gpg_available: bool,
}

/// 每个命令各自打开一次工作目录与数据库 —— 不跨命令共享状态
fn open_database() -> Result<(Workspace, Database), String> {
    let workspace = Workspace::open_default()?;
    let database = Database::open(&workspace)?;
    Ok((workspace, database))
}

#[tauri::command]
fn open_workspace() -> Result<WorkspaceInfo, String> {
    let (workspace, database) = open_database()?;
    Ok(WorkspaceInfo {
        root: workspace.root().display().to_string(),
        database_root: database.root().display().to_string(),
        meta: database.meta().clone(),
        preferences: settings::load(&workspace)?,
        protection: database.protection(),
        gpg_available: database.gpg_available(),
    })
}

#[tauri::command]
fn save_preferences(preferences: Preferences) -> Result<Preferences, String> {
    let (workspace, _) = open_database()?;
    settings::save(&workspace, preferences)
}

#[tauri::command]
fn set_protection(protection: Policy) -> Result<(), String> {
    let (_, database) = open_database()?;
    database.save_config(&Config { protection })
}

/// 解析地址栏那一行（只做语法）。空输入不是地址，返回 `None`；
/// 语法有问题时，错误里是一句给人看的话。
#[tauri::command]
fn parse_address(input: String) -> Result<Option<ParsedAddress>, String> {
    address::parse(&input)
}

/// 解析并落到仓库上：这一页在不在、是不是特殊页（`special:random` 会挑一篇落下去）
#[tauri::command]
fn resolve_address(input: String) -> Result<Option<ResolvedAddress>, String> {
    let (_, database) = open_database()?;
    database.resolve_address(&input)
}

/// 读一篇笔记（`reference` 是地址里的版本 token，`None` = 最新版）。
/// 读不到不是错误：上了锁会明说。
#[tauri::command]
fn read_note(title: String, reference: Option<String>) -> Result<Reading, String> {
    let (_, database) = open_database()?;
    database.read_note(&title, reference.as_deref())
}

#[tauri::command]
fn create_note(title: String) -> Result<String, String> {
    let (_, database) = open_database()?;
    database.create(&title)
}

/// 提交一版。`protection` 显式给出就是**换保护**；不给就照这篇当前的保护。
#[tauri::command]
fn commit_note(
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
fn load_draft(title: String) -> Result<Option<Draft>, String> {
    let (_, database) = open_database()?;
    database.load_draft(&title)
}

#[tauri::command]
fn save_draft(title: String, markdown: String) -> Result<(), String> {
    let (_, database) = open_database()?;
    database.save_draft(&title, &markdown)
}

#[tauri::command]
fn discard_draft(title: String) -> Result<bool, String> {
    let (_, database) = open_database()?;
    database.discard_draft(&title)
}

#[tauri::command]
fn list_revisions(title: String) -> Result<Vec<RevisionSummary>, String> {
    let (_, database) = open_database()?;
    database.revisions_of(&title)
}

/// 回滚到某一版（永远是**新增一个提交**）。
/// `reference` 是地址里的版本 token；`copy` 为真时复制那一版的封装（不解锁），
/// 为假时解锁那一版重写 —— 重写可以顺带指定**新的保护**（`protection`，
/// 不给就照这篇当前的保护）与它的口令（`passphrase`，只在要套对称层时用得上）。
/// 返回新版本号。
#[tauri::command]
fn rollback_note(
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
fn protection_report(
    title: String,
    reference: Option<String>,
) -> Result<resolve::ProtectionReport, String> {
    let (_, database) = open_database()?;
    database.protection_report(&title, reference.as_deref())
}

#[tauri::command]
fn delete_note(title: String) -> Result<(), String> {
    let (_, database) = open_database()?;
    database.delete(&title)
}

#[tauri::command]
fn list_notes() -> Result<Vec<NoteSummary>, String> {
    let (_, database) = open_database()?;
    database.list()
}

/// 现有的特殊页面（菜单据此生成）
#[tauri::command]
fn special_pages() -> Vec<String> {
    address::SPECIAL_PAGES
        .iter()
        .map(|page| page.to_string())
        .collect()
}

/// 编辑器预览：把 markdown 渲染成 HTML，与阅读页**同一个渲染器**
#[tauri::command]
fn render_markdown(markdown: String, title: String) -> Result<String, String> {
    let (_, database) = open_database()?;
    database.render_html(&markdown, &title)
}

/// 给某一版解锁（`reference` 是版本 token，`None` = 最新版）
#[tauri::command]
fn unlock(title: String, reference: Option<String>, passphrase: String) -> Result<(), String> {
    let (_, database) = open_database()?;
    database.unlock(&title, reference.as_deref(), &passphrase)
}

/// 忘掉这次会话里所有口令
#[tauri::command]
fn lock() {
    session::forget_all();
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .invoke_handler(tauri::generate_handler![
            open_workspace,
            save_preferences,
            set_protection,
            parse_address,
            resolve_address,
            read_note,
            create_note,
            commit_note,
            load_draft,
            save_draft,
            discard_draft,
            list_revisions,
            rollback_note,
            protection_report,
            delete_note,
            list_notes,
            special_pages,
            render_markdown,
            unlock,
            lock,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
