//! GPG 密钥：列出、导入、删除。

use crate::features::keys;

/// 钥匙串里的钥匙（新的在前）；没有 gpg 时是空列表
#[tauri::command]
pub fn gpg_keys() -> Result<Vec<keys::GpgKey>, String> {
    keys::list()
}

/// 从一份钥匙文件导入
#[tauri::command]
pub fn import_gpg_key(path: String) -> Result<keys::ImportSummary, String> {
    let bytes = std::fs::read(&path).map_err(|error| format!("读不到这个文件：{error}"))?;
    keys::import(&bytes)
}

/// 删掉一把公钥（带私钥的不动）
#[tauri::command]
pub fn delete_gpg_key(fingerprint: String) -> Result<(), String> {
    keys::delete(&fingerprint)
}
