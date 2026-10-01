//! 与 S3 兼容服务的同步：读/写设置、跑一次、把进度发出去。
//!
//! 同步是**同步跑**的（这个程序整条链子都是），但它可能要跑一会儿 ——
//! 于是每走一步都发一条 `sync-progress` 事件，界面据此写进度：界面那边是
//! "先摆加载页、看着它走"，不是"干等到天荒地老"。

use tauri::{AppHandle, Emitter};

use crate::features::sync::{self, Progress, SyncReport, SyncSettings};
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

/// 生成一把云端密钥（32 字节随机，写成 base64）并**存进设置**里。
///
/// 界面上要把这串抄给别的机器 —— 换台机器同步同一份仓库，靠的就是它。
#[tauri::command]
pub fn sync_generate_key() -> Result<String, String> {
    let workspace = open_workspace()?;
    let mut settings = sync::settings(&workspace);
    let key = sync::generate_key()?;
    settings.key = key.clone();
    settings.encrypt = true;
    sync::save_settings(&workspace, &settings)?;
    Ok(key)
}

/// 换一把云端密钥（从别的机器抄过来的那一串）
#[tauri::command]
pub fn sync_set_key(key: String) -> Result<SyncSettings, String> {
    let workspace = open_workspace()?;
    let mut settings = sync::settings(&workspace);
    let trimmed = key.trim().to_string();
    // 抄错了当场说出来，别等到同步时才炸
    if !trimmed.is_empty() {
        sync::CloudCipher::from_key(&trimmed)?;
    }
    settings.key = trimmed;
    settings.encrypt = !settings.key.is_empty();
    sync::save_settings(&workspace, &settings)?;
    Ok(settings)
}

/// 现在同步一次，返回这次做了些什么；每一步都会发 `sync-progress` 事件。
///
/// 云端那一份要不要套一层加密，由设置里的那把钥匙说了算（见 `SyncSettings`）——
/// 这里不必再过问。
#[tauri::command]
pub fn sync_now(app: AppHandle) -> Result<SyncReport, String> {
    let workspace = open_workspace()?;
    let settings = sync::settings(&workspace);

    let emitter = app.clone();
    let progress = move |step: Progress| {
        let _ = emitter.emit("sync-progress", step);
    };

    let report = sync::run(&workspace, &settings, &progress)?;
    let _ = app.emit("sync-finished", &report);
    Ok(report)
}
