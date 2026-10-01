//! 页面名的词法规则：规整、大小写、长度与保留字符；以及渲染 `[[目标]]` 用的链接解析器。
//!
//! 地址解析与仓库读写共用这一份 —— 「两个写法算不算同一篇」只有这样才只有一个答案。

use std::collections::HashSet;
use std::sync::Arc;

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

// ---------------------------------------------------------------- 链接解析

/// `[[目标]]` 的解析结果：目标存不存在（决定前端画红链还是蓝链）、
/// 是不是一条**跨站**链接（目标是别人家的页面，只出得去）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// 规范键：`<命名空间标识>:<页面名>`
    pub key: String,
    /// 显示标题（规范名 + 页面名）
    pub title: String,
    /// 目标是否存在
    pub exists: bool,
    /// 跨站链接的地址：有它就说明这一条指向外面
    pub url: Option<String>,
}

/// 渲染 `[[目标]]` 时用的解析器。
///
/// 它需要「现有的全部页面名」才能判断目标是否存在，所以由调用方在渲染前构造，
/// 通过渲染入口注入（markdown-it 没有 per-render env）。
#[derive(Clone)]
pub struct LinkResolver {
    /// 现有笔记的规范键（`<标识>:<页面名>`）
    keys: Arc<HashSet<String>>,
    /// 命名空间表：把链接里的前缀认出标识来
    table: Arc<NamespaceTable>,
    /// 当前笔记，用于展开 `/子页面`
    from: Option<ParsedTitle>,
}

impl std::fmt::Debug for LinkResolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LinkResolver")
            .field("keys", &self.keys.len())
            .field("from", &self.from)
            .finish()
    }
}

impl LinkResolver {
    pub fn new(
        keys: Arc<HashSet<String>>,
        table: Arc<NamespaceTable>,
        from: Option<ParsedTitle>,
    ) -> Self {
        Self { keys, table, from }
    }

    /// 解析一个内部链接目标；`None` 表示解析不了（留作字面文本）。
    ///
    /// 规则：
    /// - 冒号前缀跨站的 → 出一趟门（绿色，不查本仓库）；
    /// - 冒号前缀是本仓库的 → 在那个命名空间里找；
    /// - 前导斜杠 → 当前笔记的子页面；
    /// - 其余 → 主命名空间里的页面。
    pub fn resolve(&self, target: &str) -> Option<Resolved> {
        let trimmed = target.trim();
        if trimmed.is_empty() {
            return None;
        }

        // 前导 ':' 是「强制链接」语义；这里只把它脱掉，不影响目标
        let body = trimmed.strip_prefix(':').unwrap_or(trimmed).trim();

        if let Some((prefix, rest)) = body.split_once(':') {
            let found = self.table.lookup(prefix.trim())?;
            let page = check_page(&normalize(rest)).ok()?;
            let key = format!("{}:{page}", found.id);

            // 跨站：目标是别人家的页面，"在不在"这件事本仓库说了不算，所以一律当可达
            if let Some(url) = found.url_for(&page) {
                return Some(Resolved {
                    exists: true,
                    title: format!("{}:{page}", found.name),
                    key,
                    url: Some(url),
                });
            }
            // 帮助页不在仓库里，但**认得出**：`[[Help:入门]]` 该是一条蓝链
            if !found.storable && found.id != HELP_ID {
                return None;
            }
            return Some(Resolved {
                exists: self.keys.contains(&key),
                title: format!("{}:{page}", found.name),
                key,
                url: None,
            });
        }

        let (ns, page) = if let Some(rest) = body.strip_prefix('/') {
            // 相对当前笔记的子页面：拼在当前页面名后面，命名空间不变
            let from = self.from.as_ref()?;
            let suffix = normalize(rest);
            if suffix.is_empty() {
                return None;
            }
            (from.ns.clone(), format!("{}/{suffix}", from.page))
        } else {
            let name = normalize(body);
            if name.is_empty() {
                return None;
            }
            (MAIN_ID.to_string(), capitalize_first(&name))
        };

        let key = format!("{ns}:{page}");
        Some(Resolved {
            exists: self.keys.contains(&key),
            title: display_of_key(&key, &self.table),
            key,
            url: None,
        })
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
    fn links_resolve_to_capitalized_page_names() {
        let table = NamespaceTable::builtin();
        let keys = Arc::new(HashSet::from(["0:Example".to_string()]));
        let resolver = LinkResolver::new(
            keys,
            Arc::new(table.clone()),
            Some(parse("现在这一页", &table).unwrap()),
        );

        let found = resolver.resolve("example").unwrap();
        assert!(found.exists);
        assert_eq!(found.key, "0:Example");
        assert_eq!(found.title, "Example");
        assert!(found.url.is_none(), "仓库里的页面不是外链");

        let missing = resolver.resolve("没有的条目").unwrap();
        assert!(!missing.exists, "不在集合里就是红链");

        // 认不出的前缀留作字面文本：它不是地址
        assert!(resolver.resolve("foo:bar").is_none());
        // 子页面拼在当前页面后面
        assert_eq!(resolver.resolve("/子页").unwrap().key, "0:现在这一页/子页");
    }

    #[test]
    fn a_cross_site_namespace_gives_a_url() {
        let mut table = NamespaceTable::builtin();
        table
            .add(
                "zhwiki",
                Vec::new(),
                Some("https://example.org/wiki/$1".to_string()),
            )
            .unwrap();
        let resolver = LinkResolver::new(Arc::new(HashSet::new()), Arc::new(table), None);

        let found = resolver.resolve("zhwiki:平陆运河").unwrap();
        assert!(found.exists, "跨站链接一律当可达：在不在是别人家说了算");
        assert_eq!(
            found.url.as_deref(),
            Some("https://example.org/wiki/平陆运河")
        );
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
