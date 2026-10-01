// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    #[cfg(target_os = "linux")]
    fix_linux_webkit_graphics();
    refind_note_lib::run()
}

/// See https://v2.tauri.app/develop/debug/linux-graphics/
#[cfg(target_os = "linux")]
fn fix_linux_webkit_graphics() {
    if std::env::var_os("__NV_DISABLE_EXPLICIT_SYNC").is_none() {
        std::env::set_var("__NV_DISABLE_EXPLICIT_SYNC", "1");
    }
}
