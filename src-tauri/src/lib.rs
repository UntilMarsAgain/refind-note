pub mod features;
pub mod markdown;
pub mod settings;
pub mod storage;
pub mod vault;

use serde::Serialize;

use features::{browsing, changes, files, keys, maintenance};
use settings::Preferences;
use storage::{codec::Policy, session, workspace::Workspace};
use tauri::{Emitter, Manager};
use tauri_plugin_deep_link::DeepLinkExt;
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

/// 整理相关的设置（都在仓库的 `settings/repository.json` 里）
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

/// `refind://localhost/file/<名字>` → 文件页面的字节。
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

    let Some(name) = request.uri().path().strip_prefix("/file/") else {
        return missing();
    };
    let Ok((_, database)) = open_database() else {
        return missing();
    };
    // `?rev=N` 取的是历史里的那一版；不写就是最新一版
    let rev = query_value(request.uri().query(), "rev");
    let title = format!("{}:{}", vault::namespace::FILE_NAME, decode_percent(name));
    // 读不出来（没有这一页、上了锁、内容坏了）都是 404：这里只说"取不到"
    let Ok((bytes, mime)) = database.read_file(&title, rev.as_deref()) else {
        return missing();
    };

    let mime = if mime.is_empty() {
        "application/octet-stream".to_string()
    } else {
        mime
    };
    let total = bytes.len() as u64;

    // 播放器拖着进度条时会来要一段（`Range: bytes=起点-`）—— 认得出就给那一段，
    // 认不出就整份回去。视频能不能**拖动**，全看这一步
    let wanted = request
        .headers()
        .get("range")
        .and_then(|value| value.to_str().ok());
    if let Some((start, end)) = parse_range(wanted, total) {
        let slice = bytes[start as usize..=end as usize].to_vec();
        return tauri::http::Response::builder()
            .status(206)
            .header("Content-Type", mime)
            .header("Accept-Ranges", "bytes")
            .header("Content-Range", format!("bytes {start}-{end}/{total}"))
            .body(slice)
            .unwrap_or_else(|_| missing());
    }

    tauri::http::Response::builder()
        .header("Content-Type", mime)
        .header("Accept-Ranges", "bytes")
        .body(bytes)
        .unwrap_or_else(|_| missing())
}

/// `Range: bytes=起点-终点` → 闭区间 `(起点, 终点)`；认不出就是 `None`。
///
/// 只认单段、只认字节：多段的写法（`bytes=0-1,5-6`）没有播放器会用，
/// 认不了就整份回去，比猜错强。终点省略（`bytes=100-`）表示"到末尾"。
fn parse_range(header: Option<&str>, total: u64) -> Option<(u64, u64)> {
    let body = header?.trim().strip_prefix("bytes=")?.trim();
    if body.contains(',') {
        return None;
    }
    let (start, end) = body.split_once('-')?;
    let start: u64 = start.trim().parse().ok()?;
    let end: u64 = match end.trim() {
        "" => total.saturating_sub(1),
        value => value.parse().ok()?,
    };
    if start > end || end >= total {
        return None;
    }
    Some((start, end))
}

#[cfg(test)]
mod deep_link_tests {
    use super::address_from_argument;

    /// 地址是从**原始参数**里认出来的，不走 URL 解析器
    #[test]
    fn an_address_is_read_from_the_raw_argument() {
        assert_eq!(
            address_from_argument("refind://Help:首页").as_deref(),
            Some("Help:首页")
        );
        // 多一道斜杠（`refind:///…`）也常见，一样认
        assert_eq!(
            address_from_argument("refind:///Help:首页").as_deref(),
            Some("Help:首页")
        );
        // 桌面环境多半把中文百分号编码过来
        assert_eq!(
            address_from_argument("refind://Help:%E9%A6%96%E9%A1%B5").as_deref(),
            Some("Help:首页")
        );
        assert_eq!(
            address_from_argument("  refind://运河  ").as_deref(),
            Some("运河")
        );
        // 协议名大小写不挑
        assert_eq!(
            address_from_argument("REFIND://运河").as_deref(),
            Some("运河")
        );

        // 不是这条协议、或者压根没给地址
        assert_eq!(address_from_argument("/home/u/笔记.md"), None);
        assert_eq!(address_from_argument("refind://"), None);
        assert_eq!(address_from_argument("https://example.com"), None);
        // 内部取字节的地址不是"要打开哪一页"
        assert_eq!(
            address_from_argument("refind://localhost/file/%E6%A1%A5.png"),
            None
        );
    }
}

#[cfg(test)]
mod range_tests {
    use super::parse_range;

    #[test]
    fn ranges_are_read_only_when_they_make_sense() {
        // 常见三种：从头取、从中间取到末尾、取一段
        assert_eq!(parse_range(Some("bytes=0-"), 100), Some((0, 99)));
        assert_eq!(parse_range(Some("bytes=10-"), 100), Some((10, 99)));
        assert_eq!(parse_range(Some("bytes=10-20"), 100), Some((10, 20)));
        assert_eq!(parse_range(Some("bytes= 5 - 9 "), 100), Some((5, 9)));

        // 认不出的：别的单位、多段、越界、反着写、压根没给
        assert_eq!(parse_range(None, 100), None);
        assert_eq!(parse_range(Some("items=0-"), 100), None);
        assert_eq!(parse_range(Some("bytes=0-1,5-6"), 100), None);
        assert_eq!(parse_range(Some("bytes=0-100"), 100), None);
        assert_eq!(parse_range(Some("bytes=50-10"), 100), None);
        assert_eq!(parse_range(Some("bytes=abc-"), 100), None);
        // 空文件：没有可以给的段
        assert_eq!(parse_range(Some("bytes=0-"), 0), None);
    }
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

/// 从系统文件对话框选的路径收一个文件（字节由后端自己读，不走 IPC）
#[tauri::command]
fn upload_file(
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
fn update_file(
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
fn upload_bytes(request: tauri::ipc::Request<'_>) -> Result<files::Uploaded, String> {
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

/// 被 `refind://…` 唤起时，参数里那个地址（`refind://Help:首页` → `Help:首页`）。
///
/// **不走 URL 解析器**：地址里的 `:` 是命名空间分隔符，而 URL 会把它当成"端口"，
/// 端口只认数字 —— `refind://Help:首页` 在 URL 眼里根本不是个合法地址（这也正是
/// Tauri 那个 deep-link 插件收不下它的原因）。所以只认前缀，其余照原样收下。
///
/// 认两种写法：`refind://Help:首页` 与 `refind:///Help:首页`（多一道斜杠也常见）。
/// 取字节用的 `refind://localhost/file/…` 不是"要打开哪一页"，不当地址收。
fn address_from_argument(argument: &str) -> Option<String> {
    const PREFIX: &str = "refind://";
    let argument = argument.trim();
    // `get(..n)` 而不是 `[..n]`：参数可能是 `/home/u/笔记.md` 这种，
    // 第 9 个字节正落在某个字的中间，切片会当场 panic
    let head = argument.get(..PREFIX.len())?;
    if !head.eq_ignore_ascii_case(PREFIX) {
        return None;
    }
    let rest = argument[PREFIX.len()..].trim_start_matches('/').trim();
    if rest.is_empty() || rest.starts_with("localhost/") {
        return None;
    }
    // 命令行里中文多半是百分号编码过来的，交出去之前还它原样
    Some(decode_percent(rest))
}

/// 窗口可能还没建好（冷启动就是这条路），所以先记下来让界面起来之后来取；
/// 已经在跑的实例则直接收到事件（界面那会儿已经在听了）
fn deliver_address(app: &tauri::AppHandle, address: String) {
    if let Some(pending) = app.try_state::<PendingAddress>() {
        if let Ok(mut slot) = pending.0.lock() {
            *slot = Some(address.clone());
        }
    }
    let _ = app.emit("open-address", address);
}

/// 这一进程是被 `refind-note refind://…` 拉起来的吗
fn deliver_from_arguments<I: IntoIterator<Item = String>>(app: &tauri::AppHandle, args: I) {
    for argument in args {
        if let Some(address) = address_from_argument(&argument) {
            deliver_address(app, address);
        }
    }
}

/// 查询串里的一个值（`rev=3` → `3`）；没有就是 `None`
fn query_value(query: Option<&str>, wanted: &str) -> Option<String> {
    query?
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(key, _)| *key == wanted)
        .map(|(_, value)| decode_percent(value))
        .filter(|value| !value.is_empty())
}

/// 一个文件的现状（怎么存的、现在读不读得动）—— 界面据此决定"直接显示还是先解锁"。
/// `reference` 给版本 token 就看那一版
#[tauri::command]
fn file_info(key: String, reference: Option<String>) -> Result<files::FileInfo, String> {
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
fn open_file(
    app: tauri::AppHandle,
    title: String,
    reference: Option<String>,
) -> Result<String, String> {
    let (_, database) = open_database()?;
    let (bytes, _mime) = database.read_file(&title, reference.as_deref())?;
    let page = database.parse_title(&title)?.page;
    let path = stage_file(&page, &bytes)?;
    let shown = path.to_string_lossy().to_string();
    tauri_plugin_opener::OpenerExt::opener(&app)
        .open_path(shown.clone(), None::<String>)
        .map_err(|error| format!("交给系统打开失败：{error}"))?;
    // 把落点告诉界面：临时副本在哪儿，值得让人知道（尤其加密的那些）
    Ok(shown)
}

/// 把一个文件的字节落到临时目录里，返回那个路径。
///
/// 名字取页面名（后缀要留着：系统靠它挑应用）。**权限收紧到只有本人可读** ——
/// 从仓库里出来的可能是解过密的明文，临时目录别的人也可能看得到。
fn stage_file(name: &str, bytes: &[u8]) -> Result<std::path::PathBuf, String> {
    let directory = std::env::temp_dir().join("refind-note-open");
    std::fs::create_dir_all(&directory)
        .map_err(|error| format!("建不出临时目录 {}：{error}", directory.display()))?;
    let path = directory.join(name);
    crate::storage::workspace::write_bytes(&path, bytes)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .map_err(|error| format!("改权限失败：{error}"))?;
    }
    Ok(path)
}

/// 改文件的名字（页面名跟着改）
#[tauri::command]
fn rename_file(title: String, name: String) -> Result<files::FileEntry, String> {
    let (_, database) = open_database()?;
    database.rename_file(&title, &name)
}

/// 删一个文件（进回收站）
#[tauri::command]
fn delete_file(title: String) -> Result<(), String> {
    let (_, database) = open_database()?;
    database.delete_file(&title)
}

/// 另存为：把某一版复制到用户选的位置
#[tauri::command]
fn export_file(title: String, target: String) -> Result<(), String> {
    let (_, database) = open_database()?;
    database.export_file(&title, &target)
}

/// 按百分比解码一个键（粘贴上传时名字走 HTTP 头，只能是 ASCII）
fn decode_percent(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let high = (bytes[index + 1] as char).to_digit(16);
            let low = (bytes[index + 2] as char).to_digit(16);
            if let (Some(high), Some(low)) = (high, low) {
                out.push((high * 16 + low) as u8);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

/// 回收站里的条目（新的在前）
#[tauri::command]
fn list_trash() -> Result<Vec<maintenance::TrashEntry>, String> {
    let (_, database) = open_database()?;
    database.list_trash()
}

/// 还原一条：日志挪回 `objects/`，并补一版"从回收站还原"
#[tauri::command]
fn restore_note(title: String) -> Result<(), String> {
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
/// 导出某一版的 markdown 原文到用户选的位置（路径由系统保存对话框给出）
#[tauri::command]
fn export_note(title: String, reference: Option<String>, target: String) -> Result<(), String> {
    let (_, database) = open_database()?;
    database.export_note(&title, reference.as_deref(), &target)
}

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

/// 启动时被 `refind://…` 唤起的话，把那个地址交给界面（取走就清掉）
#[tauri::command]
fn take_pending_address(state: tauri::State<'_, PendingAddress>) -> Option<String> {
    state.0.lock().ok()?.take()
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

/// 等着被打开的地址（冷启动时窗口还没建好，先寄存在这里）
#[derive(Default)]
struct PendingAddress(std::sync::Mutex<Option<String>>);

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // 单实例：同一个登录会话里只跑一个重逢笔记。
        //
        // 必须**第一个**注册：第二次启动要在别的插件初始化之前就退出，
        // 只把已有窗口拉到前面 —— 两个进程去抢同一个仓库可不是闹着玩的。
        // 顺带承接 `refind://…`：系统是"再拉起一个实例、把 URL 当参数给它"，
        // 那个参数只有这里收得到。
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
            deliver_from_arguments(app, args);
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        // `refind://` 既是**我们内部**取字节用的协议（下一行那个），
        // 也是**系统认的**协议：注册之后 `refind://Help:首页` 就能把程序拉起来。
        // 内网那两个用途不冲突：内部地址写的是 `refind://localhost/file/…`，
        // 那个形式不会被当"要打开哪一页"（见 `address_from_argument`）。
        .plugin(tauri_plugin_deep_link::init())
        .register_uri_scheme_protocol("refind", |_context, request| serve_file(&request))
        .invoke_handler(tauri::generate_handler![
            open_workspace,
            save_preferences,
            set_protection,
            parse_address,
            resolve_address,
            read_note,
            export_note,
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
            update_file,
            file_info,
            open_file,
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
            take_pending_address,
        ])
        .setup(|app| {
            // 把 `refind://` 交给系统认下来：Linux 上写一份 .desktop 到用户的应用目录，
            // 再让 xdg 认这个 mime（需要 `xdg-mime` 与 `update-desktop-database` 这两条命令）。
            // 失败只记一笔：注册不上不该让程序起不来（没装那两条命令的机器就是这种）

            if let Err(error) = app.deep_link().register_all() {
                eprintln!("[deep-link] 注册 refind:// 失败：{error}");
            }
            // 冷启动：这一进程就是被 `refind-note refind://…` 拉起来的
            deliver_from_arguments(app.handle(), std::env::args().skip(1));
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
