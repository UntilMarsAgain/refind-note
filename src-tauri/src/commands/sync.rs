//! 与 S3 兼容服务的同步：读/写设置、跑一次、把进度发出去。
//!
//! **秘密不出后端**：云端密钥与 S3 私钥都不交给界面（存本地至少得碰到这台电脑，
//! 显示出来就不一定了 —— 直播、共享屏幕、随手截图都可能把它带出去）。
//! 界面只知道"配没配"；要带到别的机器上就用 [`sync_export_key`] 导成文件。

use tauri::{AppHandle, Emitter};

use crate::features::sync::{self, Progress, SyncReport, SyncSettingsPatch, SyncSettingsView};
use crate::storage::workspace::Workspace;

/// 打开工作目录（同步的设置不在数据库里，但也住在工作目录下）
fn open_workspace() -> Result<Workspace, String> {
    Workspace::open_default()
}

/// 同步的设置 —— **不含密钥**，只报"配没配"
#[tauri::command]
pub fn sync_settings() -> Result<SyncSettingsView, String> {
    Ok(sync::settings(&open_workspace()?).view())
}

/// 存同步的设置。界面交回来的那一份里没有秘密：私钥留空就是**不改**
#[tauri::command]
pub fn set_sync_settings(patch: SyncSettingsPatch) -> Result<SyncSettingsView, String> {
    let workspace = open_workspace()?;
    let mut settings = sync::settings(&workspace);
    settings.apply(patch);
    sync::save_settings(&workspace, &settings)?;
    Ok(settings.view())
}

/// 同步开没开、配没配全
#[tauri::command]
pub fn sync_ready() -> bool {
    open_workspace()
        .map(|workspace| sync::settings(&workspace).is_ready())
        .unwrap_or(false)
}

/// 生成一把新的云端密钥并启用云端加密。
///
/// 生成之后**不显示、也不返回**（见文件抬头）；要带到别的机器上，用
/// [`sync_export_key`] 导成文件 —— 那条路不会让它出现在屏幕上。
#[tauri::command]
pub fn sync_generate_key() -> Result<SyncSettingsView, String> {
    let workspace = open_workspace()?;
    let mut settings = sync::settings(&workspace);
    settings.key = sync::generate_key()?;
    settings.encrypt = true;
    // 换了钥匙，云端那些旧密文就解不开了：下一次同步把本机这份**整份重传**
    settings.reupload = true;
    sync::save_settings(&workspace, &settings)?;
    Ok(settings.view())
}

/// 用别的机器上生成的那一串密钥（粘贴进来的；同样不会显示回去）
#[tauri::command]
pub fn sync_set_key(key: String) -> Result<SyncSettingsView, String> {
    let workspace = open_workspace()?;
    let mut settings = sync::settings(&workspace);
    let trimmed = key.trim().to_string();
    if !trimmed.is_empty() {
        // 抄错了当场说出来，别等到同步时才炸
        sync::CloudCipher::from_key(&trimmed)?;
    }
    settings.key = trimmed;
    settings.encrypt = !settings.key.is_empty();
    // 换成另一把钥匙之后，云端那一份也要重传一遍
    settings.reupload = true;
    sync::save_settings(&workspace, &settings)?;
    Ok(settings.view())
}

/// 把云端密钥**写到用户选的文件里**（带到别的机器上用的那条路）。
///
/// 走系统的保存对话框：内容由后端写，界面既不显示也不经手。
#[tauri::command]
pub fn sync_export_key(target: String) -> Result<(), String> {
    let workspace = open_workspace()?;
    let settings = sync::settings(&workspace);
    if settings.key.is_empty() {
        return Err("还没有密钥可导出".to_string());
    }
    let text = format!(
        "# 重逢笔记 · 云端同步密钥\n\
         # 换台机器同步同一份仓库时，把下面这一行原样贴进「用别的密钥」。\n\
         # 这一份文件就是钥匙本身：别放进会被同步的目录，也别随手分享。\n\
         {}\n",
        settings.key
    );
    crate::storage::workspace::write_bytes(std::path::Path::new(&target), text.as_bytes())
}

/// 现在同步一次，返回这次做了些什么；每一步都会发 `sync-progress` 事件。
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
