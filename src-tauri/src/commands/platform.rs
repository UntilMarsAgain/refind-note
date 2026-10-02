//! 这台设备是什么 —— 界面偶尔要按它换一条路。

/// `"desktop"` 或 `"mobile"`（编译期就定了，不是运行时探测）。
///
/// 现在只有一处用它：另存为。桌面上弹系统保存对话框让人挑位置；手机上没那回事，
/// 统一由后端放进下载目录（见 [`crate::platform::saving`]）。
#[tauri::command]
pub fn platform_kind() -> &'static str {
    crate::platform::kind()
}
