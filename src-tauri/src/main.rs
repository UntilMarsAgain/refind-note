// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    #[cfg(target_os = "linux")]
    linux_webkit_graphics_workaround();

    refind_note_lib::run()
}

/// WebKitGTK + NVIDIA/Wayland 启动失败的官方 workaround。
///
/// 症状：`Gdk-Message: Error 71 (Protocol error) dispatching to Wayland display.`
/// 原因：WebKitGTK 的 DMABUF renderer 向 NVIDIA 驱动申请了它并不提供的 buffer format。
/// 文档：https://v2.tauri.app/develop/debug/linux-graphics/
/// 上游：https://github.com/tauri-apps/tauri/issues/9394 、#10702
///
/// 仅在外部未设置对应变量时生效，所以随时可以用环境变量接管来关闭它。
/// 若确认本机不再需要，直接注释掉 `main()` 里的调用即可。
#[cfg(target_os = "linux")]
fn linux_webkit_graphics_workaround() {
    // 官方建议的第 2 档：没有性能代价，优先靠它修掉 Error 71。
    if std::env::var_os("__NV_DISABLE_EXPLICIT_SYNC").is_none() {
        std::env::set_var("__NV_DISABLE_EXPLICIT_SYNC", "1");
    }
    // 第 3 档：放弃 DMABUF 快速路径，但能确定性消除 Error 71。
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }
}
