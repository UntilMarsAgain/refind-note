pub mod features;
pub mod markdown;
pub mod settings;
pub mod storage;
pub mod vault;

use serde::Serialize;

use features::{browsing, changes, files, keys, maintenance};
use settings::Preferences;
use storage::{codec::Policy, session, workspace::Workspace};
use vault::address::{self, ParsedAddress};
use vault::database::{Database, Meta};
use vault::namespace::Namespace;
use vault::notes::{Draft, Note, NoteSummary, Reading, RevisionSummary};
use vault::resolve::{self, ResolvedAddress};

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
    /// 仓库的整理设置：回收站留多少天、自动整理隔多少天、上次各是什么时候
    maintenance: MaintenanceInfo,
}

/// 整理相关的设置（都在 `settings/config.json` 里）
#[derive(Serialize)]
struct MaintenanceInfo {
    trash_keep_days: u64,
    gc_interval_days: u64,
    last_trash_purge: String,
    last_gc: String,
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
        maintenance: {
            let config = database.config();
            MaintenanceInfo {
                trash_keep_days: config.trash_keep_days,
                gc_interval_days: config.gc_interval_days,
                last_trash_purge: config.last_trash_purge,
                last_gc: config.last_gc,
            }
        },
    })
}

/// 改整理设置：回收站留多少天、自动整理隔多少天（都不小于 1 天）
#[tauri::command]
fn set_maintenance(trash_keep_days: u64, gc_interval_days: u64) -> Result<(), String> {
    let (_, database) = open_database()?;
    let mut config = database.config();
    config.trash_keep_days = trash_keep_days.max(1);
    config.gc_interval_days = gc_interval_days.max(1);
    database.save_config(&config)
}

#[tauri::command]
fn save_preferences(preferences: Preferences) -> Result<Preferences, String> {
    let (workspace, _) = open_database()?;
    settings::save(&workspace, preferences)
}

/// `refind://localhost/files/<键>` → 附件的字节。
///
/// 只给 webview 里的 `<img src>` 用：笔记正文里写的是相对名字，渲染之后由前端
/// 换成这个地址。键照表查，查不到、文件不在、读不动都是 404 —— **不猜路径**。
fn serve_file(request: &tauri::http::Request<Vec<u8>>) -> tauri::http::Response<Vec<u8>> {
    let missing = || {
        tauri::http::Response::builder()
            .status(404)
            .body(Vec::new())
            .expect("空响应总是拼得出来")
    };

    let Some(key) = request.uri().path().strip_prefix("/files/") else {
        return missing();
    };
    let Ok((_, database)) = open_database() else {
        return missing();
    };
    let Some(path) = database.file_path(key) else {
        return missing();
    };
    let Ok(bytes) = std::fs::read(&path) else {
        return missing();
    };

    let mime = path
        .extension()
        .map(|extension| files::mime_of(&extension.to_string_lossy().to_lowercase()))
        .unwrap_or("application/octet-stream");
    tauri::http::Response::builder()
        .header("Content-Type", mime)
        .body(bytes)
        .unwrap_or_else(|_| missing())
}

/// 钥匙串里的钥匙（新的在前）；没有 gpg 时是空列表
#[tauri::command]
fn gpg_keys() -> Result<Vec<keys::GpgKey>, String> {
    keys::list()
}

/// 从一份钥匙文件导入
#[tauri::command]
fn import_gpg_key(path: String) -> Result<keys::ImportSummary, String> {
    let bytes = std::fs::read(&path).map_err(|error| format!("读不到这个文件：{error}"))?;
    keys::import(&bytes)
}

/// 删掉一把公钥（带私钥的不动）
#[tauri::command]
fn delete_gpg_key(fingerprint: String) -> Result<(), String> {
    keys::delete(&fingerprint)
}

/// 最近的改动（跨全部笔记，新的在前）
#[tauri::command]
fn recent_changes(limit: usize, include_drafts: bool) -> Result<Vec<changes::ChangeEntry>, String> {
    let (_, database) = open_database()?;
    database.recent_changes(limit, include_drafts)
}

/// 浏览历史（新的在前）
#[tauri::command]
fn browsing_history() -> Result<Vec<browsing::Visit>, String> {
    let (_, database) = open_database()?;
    Ok(database.browsing())
}

/// 记一次访问；返回记完之后的整份清单
#[tauri::command]
fn record_visit(address: String, title: String) -> Result<Vec<browsing::Visit>, String> {
    let (_, database) = open_database()?;
    database.record_visit(&address, &title)
}

/// 清空浏览历史
#[tauri::command]
fn clear_history() -> Result<(), String> {
    let (_, database) = open_database()?;
    database.clear_browsing()
}

/// 帮助页清单（页面名与标题；正文也一并给出，页数不多）
#[tauri::command]
fn help_pages() -> Result<Vec<features::help::HelpPage>, String> {
    let (_, database) = open_database()?;
    Ok(features::help::pages(&database))
}

/// 读一页帮助（含渲染好的 HTML）
#[tauri::command]
fn read_help(page: String) -> Result<features::help::HelpPage, String> {
    let (_, database) = open_database()?;
    features::help::find(&database, &page).ok_or_else(|| format!("没有这页帮助：{page}"))
}

/// 附件清单（新的在前）
#[tauri::command]
fn list_files() -> Result<Vec<files::FileEntry>, String> {
    let (_, database) = open_database()?;
    database.list_files()
}

/// 从系统文件对话框选的路径收一个附件（字节由后端自己读，不走 IPC）
#[tauri::command]
fn upload_file(path: String) -> Result<files::Uploaded, String> {
    let (_, database) = open_database()?;
    let source = std::path::PathBuf::from(&path);
    let bytes = std::fs::read(&source).map_err(|error| format!("读不到这个文件：{error}"))?;
    let name = source
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "未命名".to_string());
    database.add_file(&name, &bytes)
}

/// 粘贴进来的字节直接走二进制通道（名字放在头里）。
///
/// 剪贴板里的文件没有路径可读，只能把字节递过来；用原始 IPC 体而不是 base64，
/// 大图才不会在编码上再翻一倍。
#[tauri::command]
fn upload_bytes(request: tauri::ipc::Request<'_>) -> Result<files::Uploaded, String> {
    let (_, database) = open_database()?;
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err("粘贴上传要走二进制通道，但没收到字节".to_string());
    };
    let name = request
        .headers()
        .get("x-file-name")
        .and_then(|value| value.to_str().ok())
        .map(files::decode_key)
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "粘贴的文件".to_string());
    database.add_file(&name, bytes)
}

/// 改附件的显示名
#[tauri::command]
fn rename_file(id: String, name: String) -> Result<files::FileEntry, String> {
    let (_, database) = open_database()?;
    database.rename_file(&id, &name)
}

/// 删一个附件
#[tauri::command]
fn delete_file(id: String) -> Result<(), String> {
    let (_, database) = open_database()?;
    database.delete_file(&id)
}

/// 另存为：把一个附件复制到用户选的位置
#[tauri::command]
fn export_file(key: String, target: String) -> Result<(), String> {
    let (_, database) = open_database()?;
    let bytes = database.read_file(&key)?;
    std::fs::write(&target, bytes).map_err(|error| format!("写不进 {target}：{error}"))
}

/// 回收站里的条目（新的在前）
#[tauri::command]
fn list_trash() -> Result<Vec<maintenance::TrashEntry>, String> {
    let (_, database) = open_database()?;
    database.list_trash()
}

/// 还原一条：日志挪回 `objects/`，并补一版"从回收站还原"
#[tauri::command]
fn restore_note(title: String) -> Result<Reading, String> {
    let (_, database) = open_database()?;
    database.restore(&title)
}

/// 永久清除一条（**不可撤销**；它引用的内容块留给整理那一轮去回收）
#[tauri::command]
fn purge_trash_entry(title: String) -> Result<(), String> {
    let (_, database) = open_database()?;
    database.purge_trash_entry(&title)
}

/// 清掉回收站里超过 `older_than_days` 天的条目（`0` = 全部）
#[tauri::command]
fn purge_trash(older_than_days: i64) -> Result<maintenance::PurgeReport, String> {
    let (_, database) = open_database()?;
    database.purge_trash(older_than_days)
}

/// 整理一遍：回收没人引用的内容块、清掉没有主的草稿槽位
#[tauri::command]
fn gc(orphan_blobs: bool, orphan_drafts: bool) -> Result<maintenance::GcReport, String> {
    let (_, database) = open_database()?;
    database.gc(orphan_blobs, orphan_drafts)
}

/// 开机自动维护：到期才做，没到期返回 null
#[tauri::command]
fn run_maintenance() -> Result<Option<maintenance::MaintenanceReport>, String> {
    let (_, database) = open_database()?;
    database.run_maintenance()
}

/// 命名空间表（标题前缀那张表）
#[tauri::command]
fn namespaces() -> Result<Vec<Namespace>, String> {
    let (_, database) = open_database()?;
    Ok(database.namespace_list())
}

/// 新建命名空间：`site` 给了就是跨站（页面不在本仓库）。返回更新后的整张表。
#[tauri::command]
fn add_namespace(
    name: String,
    aliases: Vec<String>,
    site: Option<String>,
) -> Result<Vec<Namespace>, String> {
    let (_, database) = open_database()?;
    database.add_namespace(&name, aliases, site)
}

/// 改别名与站点地址（名称与标识不动）
#[tauri::command]
fn update_namespace(
    key: String,
    aliases: Vec<String>,
    site: Option<String>,
) -> Result<Vec<Namespace>, String> {
    let (_, database) = open_database()?;
    database.update_namespace(&key, aliases, site)
}

/// 改名：不动文件，只改表里那一行与标题里的前缀
#[tauri::command]
fn rename_namespace(key: String, name: String) -> Result<Vec<Namespace>, String> {
    let (_, database) = open_database()?;
    database.rename_namespace(&key, &name)
}

/// 清空：里面的页面全部移进回收站
#[tauri::command]
fn empty_namespace(key: String) -> Result<usize, String> {
    let (_, database) = open_database()?;
    database.empty_namespace(&key)
}

/// 删除：先清空，再从表里去掉
#[tauri::command]
fn delete_namespace(key: String) -> Result<usize, String> {
    let (_, database) = open_database()?;
    database.delete_namespace(&key)
}

#[tauri::command]
fn set_protection(protection: Policy) -> Result<(), String> {
    let (_, database) = open_database()?;
    let mut config = database.config();
    config.protection = protection;
    database.save_config(&config)
}

/// 解析地址栏那一行（只做语法）。空输入不是地址，返回 `None`；
/// 语法有问题时，错误里是一句给人看的话。
#[tauri::command]
fn parse_address(input: String) -> Result<Option<ParsedAddress>, String> {
    let (_, database) = open_database()?;
    address::parse(&input, &database.namespaces())
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

/// 这一版的口令在不在本次会话里（界面上的"口令已暂存"）
#[tauri::command]
fn passphrase_stored(title: String, reference: Option<String>) -> Result<bool, String> {
    let (_, database) = open_database()?;
    database.passphrase_stored(&title, reference.as_deref())
}

/// 忘掉这一篇在这次会话里存过的口令（它的每一版）
#[tauri::command]
fn forget_passphrase(title: String) -> Result<(), String> {
    let (_, database) = open_database()?;
    let id = database.locate(&title)?;
    session::forget_note(&id);
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .register_uri_scheme_protocol("refind", |_context, request| serve_file(&request))
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
            gpg_keys,
            import_gpg_key,
            delete_gpg_key,
            help_pages,
            read_help,
            recent_changes,
            browsing_history,
            record_visit,
            clear_history,
            list_files,
            upload_file,
            upload_bytes,
            rename_file,
            delete_file,
            export_file,
            list_trash,
            restore_note,
            purge_trash_entry,
            purge_trash,
            gc,
            run_maintenance,
            set_maintenance,
            namespaces,
            add_namespace,
            update_namespace,
            rename_namespace,
            empty_namespace,
            delete_namespace,
            special_pages,
            render_markdown,
            unlock,
            lock,
            passphrase_stored,
            forget_passphrase,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
