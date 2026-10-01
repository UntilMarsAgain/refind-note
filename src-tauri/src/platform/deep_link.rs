//! `refind://`：在系统那边**注册**，在命令行上**认出来**，然后交给界面。
//!
//! 三件事各自独立：
//!
//! - 注册：让系统知道"refind:// 交给这个程序"；
//! - 解析：从命令行参数里认出地址（系统唤起一个程序的办法就是把 URL 当参数给它）；
//! - 交付：窗口没起来就先寄存，起来了就发事件。

use std::sync::Mutex;

use tauri::{AppHandle, Emitter, Manager};

use super::decode_percent;

/// 协议名（`refind://` 里的 `refind`）
const SCHEME: &str = "refind";

/// 等着被打开的地址（冷启动时窗口还没建好，先寄存在这里）
#[derive(Default)]
pub struct PendingAddress(Mutex<Option<String>>);

/// 把 `refind://…` 唤起时带的地址取走（取走就清掉）。
///
/// 界面起来之后来取一次 —— 冷启动那条路上，事件比界面先到，只能这样交棒。
#[tauri::command]
pub fn take_pending_address(state: tauri::State<'_, PendingAddress>) -> Option<String> {
    state.0.lock().ok()?.take()
}

/// 被 `refind://…` 唤起时，参数里那个地址（`refind://Help:首页` → `Help:首页`）。
///
/// **不走 URL 解析器**：地址里的 `:` 是命名空间分隔符，而 URL 会把它当成"端口"，
/// 端口只认数字 —— `refind://Help:首页` 在 URL 眼里根本不是个合法地址（这也正是
/// Tauri 那个 deep-link 插件收不下它的原因）。所以只认前缀，其余照原样收下。
///
/// 认两种写法：`refind://Help:首页` 与 `refind:///Help:首页`（多一道斜杠也常见）。
/// 取字节用的 `refind://localhost/file/…` 不是"要打开哪一页"，不当地址收。
pub fn address_from_argument(argument: &str) -> Option<String> {
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
pub fn deliver_address(app: &AppHandle, address: String) {
    if let Some(pending) = app.try_state::<PendingAddress>() {
        if let Ok(mut slot) = pending.0.lock() {
            *slot = Some(address.clone());
        }
    }
    let _ = app.emit("open-address", address);
}

/// 参数里带着地址就交出去（命令行里除了地址还可能有别的东西）
pub fn deliver_from_arguments<I: IntoIterator<Item = String>>(app: &AppHandle, args: I) {
    for argument in args {
        if let Some(address) = address_from_argument(&argument) {
            deliver_address(app, address);
        }
    }
}

/// 把 `refind://` 注册给系统。
#[cfg(target_os = "linux")]
pub fn register(app: &AppHandle) -> Result<(), String> {
    register_on_linux(app)
}

/// 其余平台交给插件（macOS 靠 Info.plist 里的声明，Windows 靠注册表）。
#[cfg(not(target_os = "linux"))]
pub fn register(app: &AppHandle) -> Result<(), String> {
    app.deep_link()
        .register_all()
        .map_err(|error| error.to_string())
}

/// Linux 上的注册：写一份 `.desktop`，再把它写成这个协议的默认处理者。
///
/// **不用插件那一步**：它写完 `.desktop` 后会调 `xdg-mime default …`，而 xdg-mime
/// 在 KDE 上要 `qtpaths`（`qt6-tools` 里的一条命令）—— 没装就在终端里吐一句
/// `qtpaths: 未找到命令`，**然后什么也没做**（它认为"没检测到 KDE 运行时"）。
/// 那两件事本身不难，直接做：写 `.desktop`（插件那份模板长什么样，这里就长什么样），
/// 再把默认关联写进 `mimeapps.list`（KDE / GNOME / XDG 都认这份文件）。
#[cfg(target_os = "linux")]
fn register_on_linux(app: &AppHandle) -> Result<(), String> {
    use std::fs;

    let exe = std::env::current_exe().map_err(|error| format!("找不到自己：{error}"))?;
    let applications = app
        .path()
        .data_dir()
        .map_err(|error| format!("找不到用户数据目录：{error}"))?
        .join("applications");
    fs::create_dir_all(&applications)
        .map_err(|error| format!("建不出 {}：{error}", applications.display()))?;

    // 一份"只认协议、不进应用菜单"的桌面项（`NoDisplay=true` 就是后者）
    let desktop_name = "refind-note-handler.desktop".to_string();
    let desktop_path = applications.join(&desktop_name);
    let name = app
        .config()
        .product_name
        .clone()
        .unwrap_or_else(|| "重逢笔记".to_string());
    let content = format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name={name}\n\
         Exec=\"{}\" %u\n\
         Terminal=false\n\
         NoDisplay=true\n\
         MimeType=x-scheme-handler/{SCHEME};\n",
        exe.display()
    );
    fs::write(&desktop_path, content)
        .map_err(|error| format!("写不出 {}：{error}", desktop_path.display()))?;

    // 让桌面环境重扫一遍目录，知道有这么个处理者。
    // 输出收走（不往终端里吐），失败也不当回事：没这条命令的机器照样能用 ——
    // 少了它只是"刚写下的关联要等下一次登录才生效"
    let _ = std::process::Command::new("update-desktop-database")
        .arg(&applications)
        .output();

    // 默认关联写进 mimeapps.list
    let config = app
        .path()
        .config_dir()
        .map_err(|error| format!("找不到用户配置目录：{error}"))?;
    fs::create_dir_all(&config).map_err(|error| format!("建不出 {}：{error}", config.display()))?;
    let path = config.join("mimeapps.list");
    let current = fs::read_to_string(&path).unwrap_or_default();
    let updated = set_default(
        &current,
        &format!("x-scheme-handler/{SCHEME}"),
        &desktop_name,
    );
    fs::write(&path, updated).map_err(|error| format!("写不出 {}：{error}", path.display()))?;

    Ok(())
}

/// 往 `mimeapps.list` 里写一条默认关联：`[Default Applications]` 段里
/// `x-scheme-handler/refind=<桌面文件名>;`。
///
/// 文件里**别的内容一个都不动** —— 那是用户自己配的一堆关联，不是我们的地盘。
/// 段落不在就补一段，键不在就插在该段末尾，已经在就换掉那一行。
#[cfg(target_os = "linux")]
fn set_default(content: &str, mime: &str, desktop: &str) -> String {
    const SECTION: &str = "[Default Applications]";
    let entry = format!("{mime}={desktop};");
    let prefix = format!("{mime}=");

    let mut lines: Vec<String> = content.lines().map(str::to_string).collect();
    // 末尾的空行先摘掉，最后统一补一个换行（不然每启动一次就多一行空行）
    while lines.last().is_some_and(|line| line.trim().is_empty()) {
        lines.pop();
    }

    match lines.iter().position(|line| line.trim() == SECTION) {
        None => {
            if !lines.is_empty() {
                lines.push(String::new());
            }
            lines.push(SECTION.to_string());
            lines.push(entry);
        }
        Some(start) => {
            // 这一段到下一个 `[` 开头的行为止
            let end = lines[start + 1..]
                .iter()
                .position(|line| line.trim_start().starts_with('['))
                .map(|offset| start + 1 + offset)
                .unwrap_or(lines.len());
            match lines[start + 1..end]
                .iter()
                .position(|line| line.trim_start().starts_with(&prefix))
            {
                Some(offset) => lines[start + 1 + offset] = entry,
                None => lines.insert(end, entry),
            }
        }
    }

    let mut text = lines.join("\n");
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
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

    #[cfg(target_os = "linux")]
    mod mimeapps {
        use super::super::set_default;

        const MIME: &str = "x-scheme-handler/refind";
        const DESKTOP: &str = "refind-note-handler.desktop";

        #[test]
        fn the_section_is_added_when_the_file_says_nothing() {
            assert_eq!(
                set_default("", MIME, DESKTOP),
                format!("[Default Applications]\n{MIME}={DESKTOP};\n")
            );
        }

        #[test]
        fn other_associations_are_left_alone() {
            let before = "[Default Applications]\ntext/plain=别的编辑器.desktop;\n\n[Added Associations]\ntext/plain=x.desktop;\n";
            let after = set_default(before, MIME, DESKTOP);

            assert!(after.contains("text/plain=别的编辑器.desktop;"), "{after}");
            assert!(after.contains("[Added Associations]"), "{after}");
            assert!(after.contains("text/plain=x.desktop;"), "{after}");
            // 插在这一段的**末尾**，没有跑到别的段里去
            let ours = after.find(MIME).unwrap();
            assert!(
                ours < after.find("[Added Associations]").unwrap(),
                "{after}"
            );
        }

        #[test]
        fn an_existing_entry_is_replaced_not_duplicated() {
            let before = format!("[Default Applications]\n{MIME}=老的.desktop;\n");
            let after = set_default(&before, MIME, DESKTOP);

            assert_eq!(after.matches(MIME).count(), 1, "{after}");
            assert!(after.contains(&format!("{MIME}={DESKTOP};")), "{after}");
            assert!(!after.contains("老的.desktop"), "{after}");
        }

        #[test]
        fn repeated_runs_do_not_pile_up_blank_lines() {
            let once = set_default("", MIME, DESKTOP);
            assert_eq!(set_default(&once, MIME, DESKTOP), once);
        }
    }
}
