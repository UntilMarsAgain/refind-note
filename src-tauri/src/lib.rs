//! Tauri 端的入口与命令。
//!
//! 命令只是 [`storage`] 的薄包装：存储、版本链、草稿的语义都在那边，前端不接触文件。
//! Markdown 的一切在 [`markdown`] 模块里，自定义语法在 [`markdown::syntax`]。

mod markdown;
mod storage;
mod title;

use std::sync::{Mutex, MutexGuard};
use storage::{
    Address, DiffResult, Draft, GcReport, LoadOutcome, Note, NoteSummary, RevisionContent, RevisionSummary,
    Vault, VaultSettings,
};
use tauri::Manager;

/// 首次运行时建的示例笔记，避免打开就是空白
const SEED_TITLE: &str = "示例笔记";
const SEED_MARKDOWN: &str = include_str!("../sample-note.md");

/// 写操作的**进程内**互斥。
///
/// 跨进程由单实例插件挡住（一台机器只允许跑一个重逢笔记），但同一个进程里 Tauri
/// 会并发跑命令：两个写操作同时「读索引 → 改 → 原子写回」时，后写的会覆盖前写的。
/// 原子写保证的是**文件完整**（不会读到半截），不是并发安全；索引虽然只是缓存、
/// 覆盖了也能重建，但当下会出现「刚建好的笔记不在列表里」这种怪现象，所以串起来。
static WRITE_LOCK: Mutex<()> = Mutex::new(());

/// 写命令开头拿一下即可。读命令不用——它们只会读到新旧两个完整版本之一。
fn write_guard() -> MutexGuard<'static, ()> {
    // 中毒说明上一个持锁的写操作 panic 了；文件本身是原子的，继续用即可
    WRITE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// 每个命令各自打开仓库。开销只是读两个小 JSON，换来的是不必跨命令共享可变状态。
fn open() -> Result<Vault, String> {
    Vault::open_default().map_err(|error| error.to_string())
}

// ---------------------------------------------------------------- 设置

#[tauri::command]
fn get_settings() -> Result<VaultSettings, String> {
    Ok(open()?.settings_view())
}

/// 只更新明确传入的字段，没传的保持不变
#[tauri::command]
fn update_settings(
    capital_links: Option<bool>,
    max_title_bytes: Option<usize>,
) -> Result<VaultSettings, String> {
    let _guard = write_guard();
    let mut vault = open()?;
    vault
        .update_settings(capital_links, max_title_bytes)
        .map_err(|error| error.to_string())?;
    Ok(vault.settings_view())
}

/// 标题校验：只有解析，不落盘。前端即时检查之外的权威判定。
#[tauri::command]
fn validate_title(title: String) -> Result<(), String> {
    open()?
        .validate_title(&title)
        .map_err(|error| error.to_string())
}

/// **回退**：把某一版的内容作为新提交写上去 —— 旧记录一条不改（回退不是撤销历史）
#[tauri::command]
fn revert_note(title: String, rev: u64, summary: Option<String>) -> Result<Note, String> {
    let _guard = write_guard();
    let vault = open()?;
    let old = vault.revision(&title, rev).map_err(|error| error.to_string())?;
    let current = vault.load(&title).map_err(|error| error.to_string())?;
    let reason = summary.unwrap_or_else(|| format!("回退到版本 {rev}"));

    vault
        .commit(&title, &old.markdown, Some(&reason), current.rev)
        .map_err(|error| error.to_string())
}

// ---------------------------------------------------------------- 笔记

#[tauri::command]
fn list_notes() -> Result<Vec<NoteSummary>, String> {
    open()?.list_notes().map_err(|error| error.to_string())
}

/// 读一篇笔记。**目标不存在不是错误**：返回 `note: None`，前端据此显示创建入口。
#[tauri::command]
fn load_note(title: String) -> Result<LoadOutcome, String> {
    open()?
        .load_outcome(&title)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn create_note(title: String) -> Result<Note, String> {
    let _guard = write_guard();
    open()?.create(&title).map_err(|error| error.to_string())
}

/// 提交：只有它会把内容写进版本链。`base_rev` 是编辑器当前基于的版本，用于冲突守卫。
#[tauri::command]
fn commit_note(
    title: String,
    markdown: String,
    summary: Option<String>,
    base_rev: u64,
) -> Result<Note, String> {
    let _guard = write_guard();
    open()?
        .commit(&title, &markdown, summary.as_deref(), base_rev)
        .map_err(|error| error.to_string())
}

/// 自动保存：往链上追加一个草稿节点，不改变当前提交
#[tauri::command]
fn save_draft(title: String, markdown: String, base_rev: u64) -> Result<(), String> {
    let _guard = write_guard();
    open()?
        .save_draft(&title, &markdown, base_rev)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn load_draft(title: String) -> Result<Option<Draft>, String> {
    open()?.load_draft(&title).map_err(|error| error.to_string())
}

#[tauri::command]
fn discard_draft(title: String) -> Result<usize, String> {
    let _guard = write_guard();
    open()?
        .discard_draft(&title)
        .map_err(|error| error.to_string())
}

/// 只清理某一篇笔记里已被提交取代的草稿节点
#[tauri::command]
fn prune_note(title: String) -> Result<usize, String> {
    let _guard = write_guard();
    open()?.prune(&title).map_err(|error| error.to_string())
}

/// 清理所有已被提交取代的草稿节点（随时可做；不做也不影响正确性）
#[tauri::command]
fn prune_drafts() -> Result<usize, String> {
    let _guard = write_guard();
    open()?.prune_all().map_err(|error| error.to_string())
}

/// 改名 / 迁移命名空间：内容不变，历史不断
#[tauri::command]
fn rename_note(from: String, to: String) -> Result<Note, String> {
    let _guard = write_guard();
    open()?
        .rename(&from, &to)
        .map_err(|error| error.to_string())
}

/// 删除：写一条删除标记，不抹除历史
#[tauri::command]
fn delete_note(title: String) -> Result<(), String> {
    let _guard = write_guard();
    open()?.delete(&title).map_err(|error| error.to_string())
}

/// 回收。两个开关各自可选，默认都不动 —— 破坏性操作，宁可手动触发。
#[tauri::command]
fn gc(orphan_blobs: bool, superseded_drafts: bool) -> Result<GcReport, String> {
    let _guard = write_guard();
    open()?
        .gc(orphan_blobs, superseded_drafts)
        .map_err(|error| error.to_string())
}

// ---------------------------------------------------------------- 历史

/// 版本历史。只回元信息（大小、摘要、类型），正文用 `note_revision` 按需取。
#[tauri::command]
fn note_history(title: String) -> Result<Vec<RevisionSummary>, String> {
    open()?.history(&title).map_err(|error| error.to_string())
}

/// 取某一个版本的正文与渲染结果（草稿也在版本序列里）
#[tauri::command]
fn note_revision(title: String, rev: u64) -> Result<RevisionContent, String> {
    open()?
        .revision(&title, rev)
        .map_err(|error| error.to_string())
}

/// 解析地址栏那一行。前端只按返回的 `kind` 分发，不自己解析。
#[tauri::command]
fn parse_address(input: String, current: Option<String>) -> Result<Address, String> {
    open()?
        .parse_address(&input, current.as_deref())
        .map_err(|error| error.to_string())
}

/// 把版本引用（数字版本号或 commit ID 缩写）解析成版本号
#[tauri::command]
fn resolve_revision(title: String, reference: String) -> Result<u64, String> {
    open()?
        .resolve_revision(&title, &reference)
        .map_err(|error| error.to_string())
}

/// 对比两个版本，逐行返回差异
#[tauri::command]
fn compare_revisions(title: String, from: u64, to: u64) -> Result<DiffResult, String> {
    open()?
        .compare(&title, from, to)
        .map_err(|error| error.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // 单实例：同一个登录会话里只允许跑一个重逢笔记。
        // 放在最前面注册，这样第二次启动会在其它插件初始化之前就退出，
        // 只把已有窗口拉到前面（而不是两个进程去抢同一个仓库）。
        //
        // 两点如实记下来：
        //   1. Linux 上它靠 D-Bus 会话名（`<identifier>.SingleInstance`）实现，
        //      所以严格说是「每个登录会话一个」，而不是整台机器一个。桌面会话里
        //      都有会话总线，日常够用。
        //   2. 只在**连会话总线地址都解析不到**时插件内部会 panic（连不上总线本身
        //      是被正常处理的）。真遇到无总线的环境，再换成自实现的锁文件方案。
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(|_app| {
            // 首次运行会在这里把 ~/.refind-note 建出来（目录 + vault.json +
            // namespaces.json），并留一篇示例笔记。失败只记日志：仓库有问题也不该
            // 让窗口打不开。
            let seeded = Vault::open_default()
                .and_then(|vault| vault.seed_if_empty(SEED_TITLE, SEED_MARKDOWN));
            match seeded {
                Ok(true) => println!("[vault] 已建立示例笔记《{SEED_TITLE}》"),
                Ok(false) => {}
                Err(error) => eprintln!("[vault] 初始化失败：{error}"),
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            update_settings,
            list_notes,
            load_note,
            create_note,
            commit_note,
            save_draft,
            load_draft,
            discard_draft,
            prune_note,
            prune_drafts,
            rename_note,
            delete_note,
            note_history,
            note_revision,
            compare_revisions,
            gc,
            validate_title,
            revert_note,
            resolve_revision,
            parse_address
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_note_renders() {
        let html = markdown::render(SEED_MARKDOWN);
        assert!(html.contains("<table>"), "表格插件应当生效");
        assert!(html.contains("<h1"), "示例里留了 H1 用于对比字号");
        assert!(html.contains("wikilink"), "示例里留了内部链接");
    }
}