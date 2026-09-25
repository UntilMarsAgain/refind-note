//! 标题与命名空间。
//!
//! 沿用 MediaWiki 的约定：`命名空间:标题`。命名空间**以英文为规范名**，允许挂其它
//! 语言的别名。
//!
//! 但有两处**刻意与 MediaWiki 不同**：
//! - 冒号前缀不是已知命名空间时，MediaWiki 会把整串（含冒号）当作主命名空间的标题；
//!   这里直接判非法——冒号在文件系统路径里是麻烦字符，而目录结构是按标题直查的。
//! - 标题里不允许 `@`：地址栏用 `标题@版本` 表达版本。

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;

/// MediaWiki 的标题长度上限
pub const MAX_TITLE_BYTES: usize = 255;

/// MediaWiki 的非法标题字符集，外加本项目自己的两条限制
/// （`:` 是命名空间分隔符，永远不属于标题本身；`@` 被地址栏的 `标题@版本` 占用）
const ILLEGAL_CHARS: &[char] = &['#', '<', '>', '[', ']', '|', '{', '}', ':', '@'];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Namespace {
    pub id: i32,
    /// 规范名；主命名空间为空串
    pub name: String,
    /// 别名（含其它语言）
    #[serde(default)]
    pub aliases: Vec<String>,
    /// 虚拟命名空间（Special / Media）不落存储
    pub storable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamespaceTable {
    pub items: Vec<Namespace>,
}

impl Default for NamespaceTable {
    fn default() -> Self {
        Self::builtin()
    }
}

impl NamespaceTable {
    /// 内建命名空间：id 0..15 是内容空间（每个内容空间 N 都配一个讨论空间 N+1），
    /// -1 / -2 是虚拟空间。
    /// 内建命名空间表。
    ///
    /// **目前只有主命名空间**：没有规范名、没有别名，所以任何非空前缀都查不到 ——
    /// 于是带冒号的标题一律判非法（见 `parse`）。
    ///
    /// 以后允许用户创建命名空间时，把新项加进这个数组即可：路径（`notes/<id>/`）、
    /// 显示标题拼装、以及「冒号前缀不是已知命名空间就报错」这条规则都已经预留好了。
    pub fn builtin() -> Self {
        Self {
            items: vec![Namespace {
                id: 0,
                name: String::new(),
                aliases: Vec::new(),
                storable: true,
            }],
        }
    }

    pub fn get(&self, id: i32) -> Option<&Namespace> {
        self.items.iter().find(|item| item.id == id)
    }

    pub fn name_of(&self, id: i32) -> Option<&str> {
        self.get(id).map(|item| item.name.as_str())
    }

    /// 按规范名或别名查找（大小写不敏感、两侧空白不影响）
    pub fn lookup(&self, prefix: &str) -> Option<&Namespace> {
        let needle = fold(prefix);
        if needle.is_empty() {
            return None;
        }
        self.items.iter().find(|item| {
            fold(&item.name) == needle
                || item.aliases.iter().any(|alias| fold(alias) == needle)
        })
    }

    /// 解析标题
    pub fn parse(&self, input: &str, capital_links: bool) -> Result<ParsedTitle, TitleError> {
        let normalized = normalize(input);
        if normalized.is_empty() {
            return Err(TitleError::Empty);
        }

        let (ns, rest) = match normalized.split_once(':') {
            Some((prefix, rest)) => match self.lookup(prefix) {
                // 命名空间命中：冒号归命名空间
                Some(item) if item.storable => (item.id, rest.trim().to_string()),
                // **与 MediaWiki 明确不同**：冒号前缀不是已知命名空间时不再退回主命名空间，
                // 而是直接判非法。两条理由：冒号在文件系统路径里是麻烦字符；
                // 地址栏还要用「标题@版本」表达版本，需要一个干净的标题字符集。
                _ => return Err(TitleError::Illegal(':')),
            },
            None => (0, normalized.clone()),
        };

        let title = if capital_links {
            capitalize_first(&rest)
        } else {
            rest
        };

        if title.is_empty() {
            return Err(TitleError::Empty);
        }
        if title.len() > MAX_TITLE_BYTES {
            return Err(TitleError::TooLong(title.len()));
        }
        if let Some(ch) = title
            .chars()
            .find(|ch| ch.is_control() || ILLEGAL_CHARS.contains(ch))
        {
            return Err(TitleError::Illegal(ch));
        }

        Ok(ParsedTitle { ns, title })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedTitle {
    pub ns: i32,
    pub title: String,
}

impl ParsedTitle {
    /// 规范键：`<namespaceId>:<title>`。用数字 id 而非名字，命名空间改名不影响键。
    pub fn key(&self) -> String {
        format!("{}:{}", self.ns, self.title)
    }

    /// 显示标题：主命名空间不加前缀
    pub fn display(&self, table: &NamespaceTable) -> String {
        match table.name_of(self.ns) {
            Some(name) if !name.is_empty() => format!("{name}:{}", self.title),
            _ => self.title.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TitleError {
    Empty,
    TooLong(usize),
    Illegal(char),
}

impl std::fmt::Display for TitleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "标题不能为空"),
            Self::TooLong(n) => write!(f, "标题过长（{n} 字节，上限 {MAX_TITLE_BYTES}）"),
            Self::Illegal(ch) => write!(f, "标题里不能出现 {ch:?}"),
        }
    }
}

impl std::error::Error for TitleError {}

// ---------------------------------------------------------------- 链接解析

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// 规范键
    pub key: String,
    /// 显示标题
    pub title: String,
    /// 目标是否存在（决定前端画红链还是蓝链）
    pub exists: bool,
}

/// 渲染 `[[目标]]` 时用的解析器。
///
/// 它需要「现有的全部规范键」才能判断目标是否存在，因此由 Vault 在渲染前构造，
/// 通过解析器的 ext 槽位注入（markdown-it-rs 没有 per-render env）。
#[derive(Clone)]
pub struct LinkResolver {
    table: Arc<NamespaceTable>,
    keys: Arc<HashSet<String>>,
    capital_links: bool,
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
        table: Arc<NamespaceTable>,
        keys: Arc<HashSet<String>>,
        capital_links: bool,
        from: Option<ParsedTitle>,
    ) -> Self {
        Self {
            table,
            keys,
            capital_links,
            from,
        }
    }

    /// 解析一个内部链接目标；返回 `None` 表示解析不了（应留作字面文本）
    pub fn resolve(&self, target: &str) -> Option<Resolved> {
        let trimmed = target.trim();
        if trimmed.is_empty() {
            return None;
        }

        // 前导 ':' 是「强制链接」语义（[[:分类:X]] 表示链接而不是归类）。
        // 我们暂不实现归类，因此只把它脱掉，不影响目标本身。
        let body = trimmed.strip_prefix(':').unwrap_or(trimmed).trim();

        let parsed = if let Some(rest) = body.strip_prefix('/') {
            // 相对当前笔记的子页面：拼在完整标题后面，命名空间不变
            let from = self.from.as_ref()?;
            let suffix = normalize(rest);
            if suffix.is_empty() {
                return None;
            }
            ParsedTitle {
                ns: from.ns,
                title: format!("{}/{suffix}", from.title),
            }
        } else {
            self.table.parse(body, self.capital_links).ok()?
        };

        let key = parsed.key();
        Some(Resolved {
            exists: self.keys.contains(&key),
            title: parsed.display(&self.table),
            key,
        })
    }
}

// ---------------------------------------------------------------- 小工具

/// 大小写与空白折叠，用于命名空间名前缀的比较
fn fold(text: &str) -> String {
    text.trim().to_lowercase()
}

/// 去首尾空白、`_` 视作空格、连续空白折叠成一个空格
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

/// 首字母大写（对 CJK 无影响）
fn capitalize_first(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) if first.is_alphabetic() => {
            first.to_uppercase().collect::<String>() + chars.as_str()
        }
        _ => text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> NamespaceTable {
        NamespaceTable::builtin()
    }

    #[test]
    fn builtin_has_only_the_main_namespace() {
        let table = table();
        assert_eq!(table.items.len(), 1);
        assert_eq!(table.items[0].id, 0);
        assert!(table.items[0].name.is_empty(), "主命名空间没有前缀");
    }

    #[test]
    fn main_namespace_has_no_prefix() {
        let parsed = table().parse("平陆运河", true).unwrap();
        assert_eq!(parsed.ns, 0);
        assert_eq!(parsed.title, "平陆运河");
        assert_eq!(parsed.key(), "0:平陆运河");
        assert_eq!(parsed.display(&table()), "平陆运河");
    }

    /// 目前只有主命名空间，所以**任何**冒号前缀都查不到，标题一律判非法。
    /// 这与 MediaWiki 不同（那边会退回主命名空间），理由见模块文档。
    #[test]
    fn colon_titles_are_rejected() {
        for input in ["Help:目录", "随便什么:内容", "Help:A:B", "Category:某分类"] {
            assert_eq!(
                table().parse(input, true),
                Err(TitleError::Illegal(':')),
                "{input} 应当被判非法"
            );
        }
    }

    #[test]
    fn at_sign_is_rejected() {
        // 地址栏用「标题@版本」表达版本，所以标题里不能有 @
        assert_eq!(table().parse("笔记@2", true), Err(TitleError::Illegal('@')));
    }

    #[test]
    fn slash_is_part_of_the_title() {
        // 斜杠原样进标题（子页面语义只在链接解析时展开）
        let parsed = table().parse("平陆运河/航道", true).unwrap();
        assert_eq!(parsed.ns, 0);
        assert_eq!(parsed.title, "平陆运河/航道");
    }

    #[test]
    fn normalization_and_capitalization() {
        let parsed = table().parse("  hello_world  ", true).unwrap();
        assert_eq!(parsed.title, "Hello world");
        let parsed = table().parse("  hello_world  ", false).unwrap();
        assert_eq!(parsed.title, "hello world");
    }

    #[test]
    fn illegal_titles_are_rejected() {
        assert_eq!(table().parse("", true), Err(TitleError::Empty));
        assert_eq!(table().parse("   ", true), Err(TitleError::Empty));
        assert_eq!(table().parse("带|竖线", true), Err(TitleError::Illegal('|')));
        assert!(matches!(
            table().parse(&"字".repeat(300), true),
            Err(TitleError::TooLong(_))
        ));
    }

    #[test]
    fn resolver_marks_missing_targets() {
        let mut keys = HashSet::new();
        keys.insert("0:存在的条目".to_string());
        let resolver = LinkResolver::new(
            Arc::new(table()),
            Arc::new(keys),
            true,
            Some(ParsedTitle {
                ns: 0,
                title: "当前笔记".into(),
            }),
        );

        let found = resolver.resolve("存在的条目").unwrap();
        assert!(found.exists);
        assert_eq!(found.key, "0:存在的条目");

        let missing = resolver.resolve("不存在的条目").unwrap();
        assert!(!missing.exists);
    }

    #[test]
    fn resolver_expands_relative_subpages() {
        let resolver = LinkResolver::new(
            Arc::new(table()),
            Arc::new(HashSet::new()),
            true,
            Some(ParsedTitle {
                ns: 0,
                title: "平陆运河".into(),
            }),
        );
        // 绝对：同命名空间
        assert_eq!(resolver.resolve("其他条目").unwrap().key, "0:其他条目");
        // 相对：拼在当前标题后面
        assert_eq!(resolver.resolve("/航道").unwrap().key, "0:平陆运河/航道");
        assert_eq!(resolver.resolve("/航道").unwrap().title, "平陆运河/航道");
    }

    /// 带冒号的目标现在解析不了，链接只能退化成字面文本
    #[test]
    fn colon_targets_do_not_resolve() {
        let resolver =
            LinkResolver::new(Arc::new(table()), Arc::new(HashSet::new()), true, None);
        assert!(resolver.resolve("Help:目录").is_none());
    }
}
