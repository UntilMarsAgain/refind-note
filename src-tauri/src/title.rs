//! 页面名的词法规则：规整、大小写、长度与保留字符。
//!
//! 地址解析与仓库读写共用这一份 —— 「两个写法算不算同一篇」只有这样才只有一个答案。

/// 虚拟命名空间 `special`：前缀大小写不敏感，规范串里写成 `Special`。
pub const SPECIAL_NAMESPACE: &str = "special";

/// 页面名最长 255 字节（按 UTF-8 计）。
pub const MAX_TITLE_BYTES: usize = 255;

/// 页面名里不允许出现的字符。
const ILLEGAL_CHARS: &[char] = &['#', '<', '>', '[', ']', '|', '{', '}', ':', '@', '$'];

/// 去首尾空白、`_` 视作空格、连续空白折叠成一个空格。
///
/// 它决定「两个写法算不算同一篇」：地址解析与仓库读写都从这里过一遍。
pub fn normalize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pending_space = false;
    for ch in text.trim().chars() {
        let ch = if ch == '_' { ' ' } else { ch };
        if ch.is_whitespace() {
            pending_space = !out.is_empty();
            continue;
        }
        if pending_space {
            out.push(' ');
            pending_space = false;
        }
        out.push(ch);
    }
    out
}

/// 首字母大写。
///
/// 用 `is_alphabetic()` + `to_uppercase()`：对 CJK 是**空操作** —— 汉字虽然是
/// `alphabetic`，但 `to_uppercase()` 返回它自己，所以中文标题不会被改。
pub fn capitalize_first(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) if first.is_alphabetic() => {
            first.to_uppercase().collect::<String>() + chars.as_str()
        }
        _ => text.to_string(),
    }
}

/// 页面名的词法检查：首字母大写、非空、不超长、没有保留字符。
pub fn check_page(page: &str) -> Result<String, String> {
    let page = capitalize_first(page);
    if page.is_empty() {
        return Err("标题不能为空".to_string());
    }
    if page.len() > MAX_TITLE_BYTES {
        return Err(format!(
            "标题过长（{} 字节，上限 {MAX_TITLE_BYTES}）",
            page.len()
        ));
    }
    if let Some(ch) = page
        .chars()
        .find(|ch| ch.is_control() || ILLEGAL_CHARS.contains(ch))
    {
        return Err(format!("标题里不能有 {ch}"));
    }
    Ok(page)
}

/// 冒号前缀不认得时的报错：空前缀与未知前缀各一句。
pub fn reject_namespace(prefix: &str) -> String {
    if prefix.is_empty() {
        "「:」前面要写命名空间名".to_string()
    } else {
        format!("没有这个命名空间：{prefix}（目前只有主命名空间）")
    }
}

/// 主命名空间里的页面名：规整 + 词法检查。
///
/// 带冒号前缀的现在都不是可存储的命名空间：`special` 是虚拟命名空间，其余不认得 ——
/// 两边各给一句说得清的错。
pub fn parse_main(title: &str) -> Result<String, String> {
    let name = normalize(title);
    if let Some((prefix, _)) = name.split_once(':') {
        let prefix = prefix.trim();
        if prefix.eq_ignore_ascii_case(SPECIAL_NAMESPACE) {
            return Err("「special」是虚拟命名空间，不能放笔记".to_string());
        }
        return Err(reject_namespace(prefix));
    }
    check_page(&name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_folds_underscores_and_whitespace() {
        assert_eq!(normalize("  hello_world  "), "hello world");
        assert_eq!(normalize("a   b"), "a b");
        // 全是下划线 → 规整完是空的
        assert_eq!(normalize("_"), "");
    }

    #[test]
    fn a_main_page_name_is_normalized_and_checked() {
        assert_eq!(parse_main("example").unwrap(), "Example");
        assert_eq!(parse_main("my_new_note").unwrap(), "My new note");
        assert!(parse_main("   ").unwrap_err().contains("标题不能为空"));
        assert!(parse_main("带<尖括号>").unwrap_err().contains("不能有 <"));
    }

    #[test]
    fn colon_prefixes_are_rejected_with_a_reason() {
        assert!(parse_main("special:all").unwrap_err().contains("虚拟命名空间"));
        assert!(parse_main("foo:bar").unwrap_err().contains("没有这个命名空间"));
        assert!(parse_main(":x").unwrap_err().contains("要写命名空间名"));
    }
}
