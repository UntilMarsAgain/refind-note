//! 与 S3 兼容服务的同步：读/写设置、跑一次、把进度发出去。
//!
//! 同步是**同步跑**的（这个程序整条链子都是），但它可能要跑一会儿 ——
//! 于是每走一步都发一条 `sync-progress` 事件，界面据此写进度：界面那边是
//! "先摆加载页、看着它走"，不是"干等到天荒地老"。

use tauri::{AppHandle, Emitter};

use crate::features::sync::{self, Plaintext, Progress, SyncReport, SyncSettings};
use crate::storage::workspace::Workspace;

/// 打开工作目录（同步的设置不在数据库里，但也住在工作目录下）
fn open_workspace() -> Result<Workspace, String> {
    Workspace::open_default()
}

/// 同步的设置（含 S3 密钥的明文 —— 这是个本地程序，界面就在本机）
#[tauri::command]
pub fn sync_settings() -> Result<SyncSettings, String> {
    Ok(sync::settings(&open_workspace()?))
}

/// 存同步的设置
#[tauri::command]
pub fn set_sync_settings(settings: SyncSettings) -> Result<SyncSettings, String> {
    let workspace = open_workspace()?;
    sync::save_settings(&workspace, &settings)?;
    Ok(settings)
}

/// 同步开没开、配没配全 —— 界面据此决定"要不要摆那一步等待"
#[tauri::command]
pub fn sync_ready() -> bool {
    open_workspace()
        .map(|workspace| sync::settings(&workspace).is_ready())
        .unwrap_or(false)
}

/// 现在同步一次，返回这次做了些什么；每一步都会发 `sync-progress` 事件。
///
/// 云端那一份要不要再套一层加密：引擎那边留了钩子（`SyncTransform`），
/// 现在传的是"原样"；等那一层做出来，这里换成带口令的实现即可。
#[tauri::command]
pub fn sync_now(app: AppHandle) -> Result<SyncReport, String> {
    let workspace = open_workspace()?;
    let settings = sync::settings(&workspace);

    let emitter = app.clone();
    let progress = move |step: Progress| {
        let _ = emitter.emit("sync-progress", step);
    };

    let report = sync::run(&workspace, &settings, &Plaintext, &progress)?;
    let _ = app.emit("sync-finished", &report);
    Ok(report)
}
