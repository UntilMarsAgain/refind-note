mod address;

use address::ParsedAddress;

/// 解析地址栏那一行。空输入不是地址，返回 `None`；
/// 语法有问题时，错误里是一句给人看的话。
#[tauri::command]
fn parse_address(input: String) -> Result<Option<ParsedAddress>, String> {
    address::parse(&input)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![parse_address])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
