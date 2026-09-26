//! Tauri 端的入口与命令。
//!
//! 命令只是 [`storage`] 的薄包装：存储、版本链、草稿的语义都在那边，前端不接触文件。
//! Markdown 的一切在 [`markdown`] 模块里，自定义语法在 [`markdown::syntax`]。

mod command;
mod markdown;
mod storage;
mod tasks;
mod title;

use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};
use storage::{
    Address, CommandInfo, Namespace, DiffResult, Draft, FileEntry, GcReport, LoadOutcome,
    MaintenanceReport, Note, NoteSummary, PurgeReport, RevisionContent, RevisionSummary,
    TrashEntry, Vault, VaultSettings, DebugReport, RenderReport,
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
fn list_files() -> Result<Vec<FileEntry>, String> {
    Ok(open()?.list_files().map_err(|error| error.to_string())?)
}

/// 收进一个附件。`path` 是系统文件选择器给的本机路径 —— **Rust 直接读**，
/// 不让字节走一遍 IPC：一张几 MB 的图转成 JSON 数组再传，既不必要也慢。
#[tauri::command]
fn upload_file(path: String) -> Result<FileEntry, String> {
    let source = PathBuf::from(&path);
    let bytes = fs::read(&source).map_err(|error| format!("读不到这个文件：{error}"))?;
    let name = source
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "未命名".to_string());
    open()?
        .add_file(&name, &bytes)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn delete_file(id: String) -> Result<(), String> {
    open()?.delete_file(&id).map_err(|error| error.to_string())
}

/// 取附件：`refind://localhost/files/<id>`。
///
/// 用自定义方案而不是 Tauri 的资产协议：这里的映射完全由程序掌握 —— 请求里只有**标识**，
/// 真正的路径由 `files.json` 决定，所以任何形式的路径穿越都不成立；也不必给资产协议
/// 配一个能覆盖整个仓库的作用域。
fn serve_file(request: &tauri::http::Request<Vec<u8>>) -> tauri::http::Response<Vec<u8>> {
    let not_found = || {
        tauri::http::Response::builder()
            .status(404)
            .body(Vec::new())
            .unwrap_or_else(|_| tauri::http::Response::new(Vec::new()))
    };

    let Some(id) = request.uri().path().strip_prefix("/files/") else {
        return not_found();
    };
    let Ok(vault) = open() else {
        return not_found();
    };
    let Ok(Some(path)) = vault.file_path_by_id(id) else {
        return not_found();
    };
    let Ok(bytes) = fs::read(&path) else {
        return not_found();
    };
    let extension = path
        .extension()
        .map(|value| value.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    tauri::http::Response::builder()
        .status(200)
        .header("Content-Type", storage::mime_of(&extension))
        .body(bytes)
        .unwrap_or_else(|_| not_found())
}

#[tauri::command]
fn get_settings() -> Result<VaultSettings, String> {
    Ok(open()?.settings_view())
}

/// 只更新明确传入的字段，没传的保持不变
#[tauri::command]
fn update_settings(
    capital_links: Option<bool>,
    max_title_bytes: Option<usize>,
    delta_chain_limit: Option<usize>,
    trash_keep_days: Option<u64>,
    gc_interval_days: Option<u64>,
    theme: Option<String>,
    accent: Option<String>,
    reading_width: Option<u32>,
    zoom: Option<f64>,
) -> Result<VaultSettings, String> {
    let _guard = write_guard();
    let mut vault = open()?;
    vault
        .update_settings(
            capital_links,
            max_title_bytes,
            delta_chain_limit,
            trash_keep_days,
            gc_interval_days,
            theme,
            accent,
            reading_width,
            zoom,
        )
        .map_err(|error| error.to_string())?;
    Ok(vault.settings_view())
}

/// 现有的特殊页面。**哪个页面存在由后端说了算**，前端只负责显示 ——
/// 否则每加一个页面都要在前端再登记一次，那就是两个真相。
#[tauri::command]
fn special_pages() -> Vec<String> {
    crate::storage::SPECIAL_PAGES
        .iter()
        .map(|page| page.to_string())
        .collect()
}

/// 按 `@no-command` 读一篇指令页面（正文包成代码块）。
///
/// 单独一条命令，而不是给 `load_note` 加参数：现有调用点因此一个都不用动，
/// 前端也只在"地址里带 @no-command"这一种情况下走它。
#[tauri::command]
fn load_note_no_command(title: String) -> Result<LoadOutcome, String> {
    open()?
        .load_code_blocked(&title)
        .map_err(|error| error.to_string())
}

/// 某一页的指令信息（`@no-command` 顶部提示用）
#[tauri::command]
fn command_info(title: String) -> Result<Option<CommandInfo>, String> {
    open()?
        .command_info(&title)
        .map_err(|error| error.to_string())
}

/// 命名空间表（设置页用）
#[tauri::command]
fn namespaces() -> Result<Vec<Namespace>, String> {
    Ok(open()?.namespaces())
}

/// 新增一个命名空间
#[tauri::command]
fn add_namespace(
    name: String,
    aliases: Vec<String>,
    site: Option<String>,
) -> Result<Vec<Namespace>, String> {
    let _guard = write_guard();
    let mut vault = open()?;
    vault
        .add_namespace(&name, aliases, site)
        .map_err(|error| error.to_string())?;
    Ok(vault.namespaces())
}

/// 编辑页「状态」面板要的编译报告：对给定文本渲染一次，并报告这次渲染的来龙去脉。
///
/// 按需调用（面板上的「收集」）：预览本身走 `render_markdown`，这条命令不跟着每次输入跑。
#[tauri::command]
fn render_report(markdown: String, address: Option<String>) -> Result<RenderReport, String> {
    let vault = open()?;
    Ok(vault.render_report(&markdown, address.as_deref()))
}

/// 诊断报告（`special:debug`）：把仓库现在是什么样摊开成可读的分段。
///
/// 它是一项功能而不是调试残留 —— 排查"模板取不到""页面去哪了""占了多少空间"时，
/// 这里是唯一能一次看全的地方。`address` 给当前地址，报告里会多一段"当前页"。
#[tauri::command]
fn debug_report(address: Option<String>) -> Result<DebugReport, String> {
    let vault = open()?;
    Ok(vault.debug_report(address.as_deref()))
}

/// 改一个命名空间的别名与站点地址（别名增删、站址修改都用它，一次给一整套）
#[tauri::command]
fn update_namespace(
    key: String,
    aliases: Vec<String>,
    site: Option<String>,
) -> Result<Vec<Namespace>, String> {
    let _guard = write_guard();
    let mut vault = open()?;
    vault
        .update_namespace(&key, aliases, site)
        .map_err(|error| error.to_string())?;
    Ok(vault.namespaces())
}

/// 给命名空间改名（只改表里一行，不动文件）
#[tauri::command]
fn rename_namespace(key: String, name: String) -> Result<Vec<Namespace>, String> {
    let _guard = write_guard();
    let mut vault = open()?;
    vault
        .rename_namespace(&key, &name)
        .map_err(|error| error.to_string())?;
    Ok(vault.namespaces())
}

/// 清空一个命名空间（页面进回收站）
#[tauri::command]
fn empty_namespace(key: String) -> Result<Vec<Namespace>, String> {
    let _guard = write_guard();
    let vault = open()?;
    vault
        .empty_namespace(&key)
        .map_err(|error| error.to_string())?;
    Ok(vault.namespaces())
}

/// 删除一个命名空间（先清空，再从表里去掉；不可找回）
#[tauri::command]
fn delete_namespace(key: String) -> Result<Vec<Namespace>, String> {
    let _guard = write_guard();
    let mut vault = open()?;
    vault
        .delete_namespace(&key)
        .map_err(|error| error.to_string())?;
    Ok(vault.namespaces())
}

/// 回收站清单（`special:trash` 用它）
#[tauri::command]
fn list_trash() -> Result<Vec<TrashEntry>, String> {
    open()?.list_trash().map_err(|error| error.to_string())
}

/// 从回收站还原一篇笔记
#[tauri::command]
fn restore_note(title: String) -> Result<Note, String> {
    let _guard = write_guard();
    let vault = open()?;
    vault
        .restore_note(&title)
        .map_err(|error| error.to_string())
}

/// 立即清除回收站里的一条（不等保留期）
#[tauri::command]
fn purge_trash_entry(title: String) -> Result<(), String> {
    let _guard = write_guard();
    let vault = open()?;
    vault
        .purge_trash_entry(&title)
        .map_err(|error| error.to_string())
}

/// 提交一次内容块回收（后台执行；进度与结果见任务栏）
#[tauri::command]
fn submit_gc(orphan_blobs: bool, superseded_drafts: bool, purge_trash_first: bool) -> u64 {
    tasks::submit("回收内容块", move || {
        let _guard = write_guard();
        let vault = open()?;

        // 先清空回收站：那些笔记的内容块此时才成为孤块，接着这次回收一并清掉。
        // `purge_trash(0)` 的语义就是"全部清掉"（0 天前的都算）。
        let purged = if purge_trash_first {
            Some(
                vault
                    .purge_trash(0)
                    .map_err(|error| error.to_string())?
                    .removed,
            )
        } else {
            None
        };

        let report = vault
            .gc(orphan_blobs, superseded_drafts)
            .map_err(|error| error.to_string())?;

        let recycled = format!(
            "回收内容块 {} 个、草稿节点 {} 个，释放 {} 字节",
            report.removed_blobs, report.removed_drafts, report.freed_bytes
        );
        Ok(match purged {
            Some(count) => format!("清空回收站 {count} 条；{recycled}"),
            None => recycled,
        })
    })
}

/// 提交一次数据库维护。
///
/// **没到点就返回 `None`、不建任务**：空转的任务会让任务栏每次都弹一条"数据库维护"，
/// 让人以为它在干活。判断在前台做一次（只读一次配置，很便宜）。
#[tauri::command]
fn submit_maintenance() -> Result<Option<u64>, String> {
    if !open()?
        .maintenance_pending()
    {
        return Ok(None);
    }

    Ok(Some(tasks::submit("数据库维护", move || {
        let _guard = write_guard();
        let mut vault = open()?;
        let report = vault
            .run_maintenance()
            .map_err(|error| error.to_string())?;

        let purged = report.purged.map(|item| item.removed).unwrap_or(0);
        let blobs = report.gc.map(|item| item.removed_blobs).unwrap_or(0);
        Ok(if purged == 0 && blobs == 0 {
            "本次没有需要清理的内容".to_string()
        } else {
            format!("清理回收站 {purged} 条，回收内容块 {blobs} 个")
        })
    })))
}

/// 任务表快照（底部任务栏用）
#[tauri::command]
fn list_tasks() -> Vec<tasks::Task> {
    tasks::list()
}

/// 自动维护：到点了就清回收站、回收内容块。前端在启动时调一次。
#[tauri::command]
fn run_maintenance() -> Result<MaintenanceReport, String> {
    let _guard = write_guard();
    let mut vault = open()?;
    vault
        .run_maintenance()
        .map_err(|error| error.to_string())
}

/// 清理回收站：删掉超过 `olderThanDays` 天的条目，并顺手回收内容块
#[tauri::command]
fn purge_trash(older_than_days: i64) -> Result<PurgeReport, String> {
    let _guard = write_guard();
    let vault = open()?;
    vault
        .purge_trash(older_than_days)
        .map_err(|error| error.to_string())
}

/// 判断一段 markdown 是不是指令页面，返回指令短名。
///
/// 前端用它决定"提交之后落在哪个地址"：指令页面要落在 `@no-command` 上，
/// 否则刚提交完就被自己的重定向带走。**判断规则只有一处**（`command.rs`），
/// 前端不自己解释 `$$COMMAND$$`。
#[tauri::command]
fn command_kind(markdown: String) -> Option<String> {
    crate::command::parse(&markdown).kind().map(str::to_string)
}

/// 渲染预览：与阅读视图同一个渲染器（编辑器右侧的预览区用它）
#[tauri::command]
fn render_markdown(markdown: String) -> Result<String, String> {
    Ok(open()?.render(&markdown))
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
        .revision_code_blocked(&title, rev)
        .map_err(|error| error.to_string())
}

/// 解析地址栏那一行。前端只按返回的 `kind` 分发，不自己解析。
#[tauri::command]
fn parse_address(input: String) -> Result<Address, String> {
    open()?
        .parse_address(&input)
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
        .register_uri_scheme_protocol("refind", |_ctx, request| serve_file(&request))
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
        .plugin(tauri_plugin_dialog::init())
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
            list_files,
            upload_file,
            delete_file,
            special_pages,
            render_markdown,
            load_note_no_command,
            command_kind,
            command_info,
            namespaces,
            add_namespace,
            rename_namespace,
            debug_report,
            render_report,
            update_namespace,
            empty_namespace,
            delete_namespace,
            list_trash,
            purge_trash,
            restore_note,
            purge_trash_entry,
            run_maintenance,
            submit_gc,
            submit_maintenance,
            list_tasks,
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

    /// 示例笔记是用户第一眼看到的东西：写坏一份，等于新仓库开箱就是坏的。
    ///
    /// 这里不硬编码小节清单，而是**拿示例自己的目录去对渲染结果** ——
    /// 目录指向的锚点必须真的存在，否则页内跳转就是坏的，而那种坏法肉眼很难发现。
    #[test]
    fn sample_note_is_consistent() {
        let html = markdown::render(SEED_MARKDOWN);

        assert!(html.contains("<table>"), "表格插件应当生效");
        assert!(html.contains("wikilink"), "示例里应当演示内部链接");
        // 示例里演示了模板块：必须真的渲染成引用块，而不是漏成代码块或普通段落
        assert!(
            html.contains(r#"<blockquote class="quote">"#),
            "示例里的 ::quote 应当生效"
        );
        assert!(html.contains("quote__origin"), "示例里的署名应当渲染出来");

        // 示例里演示了每个内置模板：演示要是渲染不出来，示例就退化成死文字
        for marker in [
            r#"<aside class="aside">"#,
            r#"<table class="fields">"#,
            // banner 可能带 style（指定了底色），所以只匹配到类名
            r#"<p class="banner""#,
            r#"<figure class="image image--center">"#,
            r#"<pre class="template-code""#,
        ] {
            assert!(html.contains(marker), "示例里应当渲染出 {marker}");
        }
        // 图片指向真实存在的静态资源（tauri.svg 早就不在了）
        assert!(html.contains("src=\"/logo.svg\""), "示例里的图片应当指向 logo.svg");

        let mut checked = 0;
        for line in SEED_MARKDOWN.lines() {
            let Some(rest) = line.strip_prefix("- [") else {
                continue;
            };
            let Some((_, anchor)) = rest.split_once("](#") else {
                continue;
            };
            let anchor = anchor.trim_end_matches(')');
            assert!(
                html.contains(&format!(r#"id="{anchor}""#)),
                "目录指向的小节不存在：{anchor}"
            );
            checked += 1;
        }
        assert!(checked >= 5, "示例应当有目录，且每一节都能跳");

        // 示例笔记本身必须是普通页面：指令标记只许出现在代码块里（第一行不许是它）
        let first_line = SEED_MARKDOWN.lines().next().unwrap_or("").trim_end();
        assert_ne!(
            first_line, "$$COMMAND$$",
            "示例笔记不能是指令页面（第一行被写成了指令标记）"
        );
    }
}
