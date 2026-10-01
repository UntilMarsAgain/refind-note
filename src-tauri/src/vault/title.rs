//! 页面名的词法规则：规整、大小写、长度与保留字符；以及渲染 `[[目标]]` 用的链接解析器。
//!
//! 地址解析与仓库读写共用这一份 —— 「两个写法算不算同一篇」只有这样才只有一个答案。

use crate::vault::namespace::{NamespaceTable, HELP_ID, MAIN_ID, SPECIAL_ID};

/// 页面名最长 255 字节（按 UTF-8 计）。
pub const MAX_TITLE_BYTES: usize = 255;

/// 页面名里不允许出现的字符。命名空间名走的是同一张表（见 `namespace.rs`）。
pub(crate) const ILLEGAL_CHARS: &[char] = &['#', '<', '>', '[', ']', '|', '{', '}', ':', '@', '$'];

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
pub fn reject_namespace(prefix: &str, table: &NamespaceTable) -> String {
    if prefix.is_empty() {
        return "「:」前面要写命名空间名".to_string();
    }
    let known: Vec<String> = table
        .items
        .iter()
        .filter(|item| !item.name.is_empty())
        .map(|item| item.name.clone())
        .collect();
    format!(
        "没有这个命名空间：{prefix}（现在有：主、{}）",
        known.join("、")
    )
}

/// 一个标题拆开的样子：命名空间标识 + 页面名。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedTitle {
    /// 命名空间**标识**（不是名字）：磁盘与键都用它
    pub ns: String,
    /// 页面名（不含前缀）
    pub page: String,
}

impl ParsedTitle {
    /// 规范键：`<标识>:<页面名>`。存量、目录、比较都用它
    pub fn key(&self) -> String {
        format!("{}:{}", self.ns, self.page)
    }

    /// 显示标题：主命名空间不带前缀，其余写成 `名称:页面名`
    pub fn display(&self, table: &NamespaceTable) -> String {
        if self.ns == MAIN_ID {
            self.page.clone()
        } else {
            format!("{}:{}", table.name_of(&self.ns), self.page)
        }
    }
}

/// 标题 → 命名空间 + 页面名。
///
/// 没有冒号就是主命名空间；有冒号时前缀要在表里认得出（规范名或别名都认，
/// 大小写与首尾空白不影响），而且那个命名空间得是**可存储**的 ——
/// `special` 是虚拟的、跨站命名空间的页面在别人家，都放不进本仓库。
pub fn parse(title: &str, table: &NamespaceTable) -> Result<ParsedTitle, String> {
    let name = normalize(title);

    let Some((prefix, rest)) = name.split_once(':') else {
        return Ok(ParsedTitle {
            ns: MAIN_ID.to_string(),
            page: check_page(&name)?,
        });
    };

    let prefix = prefix.trim();
    if prefix.is_empty() {
        return Err(reject_namespace(prefix, table));
    }
    let Some(found) = table.lookup(prefix) else {
        return Err(reject_namespace(prefix, table));
    };
    if !found.storable {
        return Err(match found.id.as_str() {
            SPECIAL_ID => format!("「{}」是虚拟命名空间，里面的页面由程序提供", found.name),
            HELP_ID => format!("「{}」是帮助内容，不作为笔记保存", found.name),
            _ => format!("「{}」是跨站命名空间，里面的页面不在本仓库", found.name),
        });
    }

    Ok(ParsedTitle {
        ns: found.id.clone(),
        page: check_page(rest.trim())?,
    })
}

/// 规范键 → 显示标题。键就是我们自己拼的，前缀一定认得出；
/// 认不出（表被改过、键是别处来的）就原样还回去，不假装认得。
pub fn display_of_key(key: &str, table: &NamespaceTable) -> String {
    match key.split_once(':') {
        None => key.to_string(),
        Some((ns, page)) => {
            if ns == MAIN_ID {
                return page.to_string();
            }
            match table.get(ns) {
                Some(item) if item.name.is_empty() => page.to_string(),
                Some(item) => format!("{}:{}", item.name, page),
                None => key.to_string(),
            }
        }
    }
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
        let table = NamespaceTable::builtin();
        assert_eq!(parse("example", &table).unwrap().page, "Example");
        assert_eq!(parse("my_new_note", &table).unwrap().page, "My new note");
        assert!(parse("   ", &table).unwrap_err().contains("标题不能为空"));
        assert!(parse("带<尖括号>", &table)
            .unwrap_err()
            .contains("不能有 <"));
    }

    #[test]
    fn colon_prefixes_are_rejected_with_a_reason() {
        let table = NamespaceTable::builtin();
        assert!(parse("special:all", &table)
            .unwrap_err()
            .contains("虚拟命名空间"));
        assert!(parse("foo:bar", &table)
            .unwrap_err()
            .contains("没有这个命名空间"));
        assert!(parse(":x", &table).unwrap_err().contains("要写命名空间名"));
    }
}
