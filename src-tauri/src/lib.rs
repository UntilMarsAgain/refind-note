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

//! 重逢笔记的后端入口。
//!
//! 这一层只做**装配**：声明模块、把插件与命令表接上、跑起来。
//! 具体的东西各归各处：
//!
//! - [`commands`]：前端叫得到的那些命令，按域分文件；
//! - [`platform`]：与操作系统打交道的那几件事（协议注册、命令行参数、取字节服务）；
//! - [`vault`]：仓库本身 —— 地址、命名空间、笔记、文件；
//! - [`storage`]：字节怎么存（blob 仓、封装、会话口令）；
//! - [`markdown`]：markdown 与模板；
//! - [`features`]：回收站、整理、最近更改、GPG 密钥、帮助页这些围着仓库转的功能。

pub mod commands;
pub mod features;
pub mod keymap;
pub mod markdown;
pub mod platform;
pub mod settings;
pub mod storage;
pub mod vault;

use storage::workspace::Workspace;
use tauri::Manager;
use vault::database::Database;

/// 每个命令各自打开一次工作目录与数据库 —— 不跨命令共享状态。
///
/// 唯一一处"打开仓库"的地方：命令层与平台层都从这里拿，谁也不自己拼路径。
pub(crate) fn open_database() -> Result<(Workspace, Database), String> {
    let workspace = Workspace::open_default()?;
    let database = Database::open(&workspace)?;
    Ok((workspace, database))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default();

    // 单实例：同一个登录会话里只跑一个重逢笔记。
    //
    // 必须**第一个**注册：第二次启动要在别的插件初始化之前就退出，
    // 只把已有窗口拉到前面 —— 两个进程去抢同一个仓库可不是闹着玩的。
    // 顺带承接 `refind://…`：系统是"再拉起一个实例、把 URL 当参数给它"，
    // 那个参数只有这里收得到。
    //
    // **手机上没这回事**：那边一个程序本来就只有前台这一个实例，也没有"第二次启动"
    // 这码事（那个插件也只做桌面）—— 所以这里分平台拼。
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.unminimize();
            let _ = window.set_focus();
        }
        platform::deep_link::deliver_from_arguments(app, args);
    }));

    builder
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        // `refind://` 既是**我们内部**取字节用的协议（下一行那个），
        // 也是**系统认的**协议：注册之后 `refind://Help:首页` 就能把程序拉起来。
        // 两个用途不冲突：内部地址写的是 `refind://localhost/file/…`，
        // 那个形式不会被当成"要打开哪一页"（见 `platform::deep_link`）。
        .plugin(tauri_plugin_deep_link::init())
        .register_uri_scheme_protocol("refind", |_context, request| {
            platform::protocol::serve_file(&request)
        })
        // 待打开地址先寄存在这里：窗口还没建好时（冷启动）也得有个地方放
        .manage(platform::deep_link::PendingAddress::default())
        .invoke_handler(tauri::generate_handler![
            commands::diagnostics::diagnostics,
            commands::platform::platform_kind,
            commands::workspace::open_workspace,
            commands::workspace::save_preferences,
            commands::workspace::load_keymap,
            commands::workspace::save_keymap,
            commands::workspace::set_protection,
            commands::workspace::set_maintenance,
            commands::notes::parse_address,
            commands::notes::resolve_address,
            commands::notes::read_note,
            commands::notes::export_note,
            commands::notes::note_print_html,
            commands::backup::export_repository,
            commands::notes::create_note,
            commands::notes::commit_note,
            commands::notes::load_draft,
            commands::notes::save_draft,
            commands::notes::discard_draft,
            commands::notes::list_revisions,
            commands::notes::rollback_note,
            commands::notes::protection_report,
            commands::notes::delete_note,
            commands::notes::list_notes,
            commands::notes::render_markdown,
            commands::notes::unlock,
            // 页内解锁：`::decrypt` 上提交之后调的那一条（见 commands/lock.rs）
            commands::lock::resolve_decrypt,
            commands::notes::lock,
            commands::notes::passphrase_stored,
            commands::notes::forget_passphrase,
            commands::files::list_files,
            commands::files::upload_file,
            commands::files::update_file,
            commands::files::upload_bytes,
            commands::files::file_info,
            commands::files::open_file,
            commands::files::rename_file,
            commands::files::delete_file,
            commands::files::export_file,
            commands::help::help_pages,
            commands::help::all_help_pages,
            commands::help::read_help,
            commands::keys::gpg_keys,
            commands::keys::import_gpg_key,
            commands::keys::delete_gpg_key,
            commands::maintenance::list_trash,
            commands::maintenance::restore_note,
            commands::maintenance::purge_trash_entry,
            commands::maintenance::purge_trash,
            commands::maintenance::gc,
            commands::maintenance::run_maintenance,
            commands::namespaces::namespaces,
            commands::namespaces::add_namespace,
            commands::namespaces::update_namespace,
            commands::namespaces::rename_namespace,
            commands::namespaces::empty_namespace,
            commands::namespaces::delete_namespace,
            commands::activity::recent_changes,
            commands::activity::browsing_history,
            commands::activity::record_visit,
            commands::activity::clear_history,
            commands::activity::special_pages,
            commands::sync::sync_settings,
            commands::sync::sync_generate_key,
            commands::sync::sync_set_key,
            commands::sync::sync_export_key,
            commands::sync::sync_copy_key,
            commands::sync::set_sync_settings,
            commands::sync::sync_ready,
            commands::sync::sync_now,
            platform::deep_link::take_pending_address,
        ])
        .setup(|app| {
            // 手机上不存在"用户主目录"这回事：仓库与临时文件都放进**程序自己的目录**
            // （Android 上是 `/data/user/0/<包名>/`，卸载才没）。这件事得在窗口起来之前
            // 定下来 —— 第一条命令进来就要用（见 `storage::workspace::install_root`）。
            #[cfg(mobile)]
            {
                if let Ok(root) = app.path().app_data_dir() {
                    storage::workspace::install_root(root.join("refind-note"));
                }
                if let Ok(cache) = app.path().app_cache_dir() {
                    platform::staging::install_scratch(cache.join("refind-note-open"));
                }
            }

            // 把 `refind://` 交给系统认下来（Linux 上由我们自己写 .desktop 与 mimeapps.list，
            // 见 `platform::deep_link` 里那段说明）。失败只记一笔：注册不上不该让程序起不来
            if let Err(error) = platform::deep_link::register(app.handle()) {
                eprintln!("[deep-link] 注册 refind:// 失败：{error}");
            }
            // 冷启动：这一进程就是被 `refind-note refind://…` 拉起来的
            platform::deep_link::deliver_from_arguments(app.handle(), std::env::args().skip(1));
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
