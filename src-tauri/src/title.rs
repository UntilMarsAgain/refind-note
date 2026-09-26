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
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// MediaWiki 的标题长度上限
pub const MAX_TITLE_BYTES: usize = 255;

/// 主命名空间的**占位标识**。
///
/// 主命名空间没有前缀，但键（`<id>:<标题>`）与目录（`notes/<id>/`）总得有个名字，
/// 于是用 `"0"` 占位 —— 它只是"这里没有前缀"的记号，**不是数字序号**。
pub const MAIN_NS: &str = "0";

/// 虚拟命名空间：页面由程序提供，不落存储。
pub const SPECIAL_NS: &str = "special";

/// 模板命名空间：用户自己写的模板。
///
/// 与主命名空间一样**可存储**（里面就是普通笔记，一样有历史与草稿），也一样**不能删**——
/// 它承载的是所有笔记都可能引用到的定义，删掉等于把别人的页面一起弄坏。
/// 里面以 `.css` / `.html` 结尾的页面另有特判（见 `crate::markdown`）。
pub const TEMPLATE_NS: &str = "template";

/// 命名空间标识。
///
/// 它**是字符串**（`special` / `help` / …），因为用户看到的、写在地址里的就是名字；
/// 主命名空间用 [`MAIN_NS`] 占位。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum NamespaceIdRepr {
    Text(String),
    Number(i64),
}

/// 反序列化标识：**同时接受字符串与数字** —— 早期版本写的是数字，
/// 旧文件不该因为这次模型修正就读不出来。
fn de_namespace_id<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(match NamespaceIdRepr::deserialize(deserializer)? {
        NamespaceIdRepr::Text(text) => text,
        NamespaceIdRepr::Number(number) => number.to_string(),
    })
}

/// MediaWiki 的非法标题字符集，外加本项目自己的两条限制
/// （`:` 是命名空间分隔符，永远不属于标题本身；`@` 被地址栏的 `标题@版本` 占用）
// `@` 是版本引用、`!` 是模式后缀，所以都不允许出现在标题里 ——
// 地址语法才能靠字符本身切分，不必猜。
const ILLEGAL_CHARS: &[char] = &['#', '<', '>', '[', ']', '|', '{', '}', ':', '@', '$'];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Namespace {
    /// 标识：**稳定键**，与名字分开。
    ///
    /// 磁盘目录（`notes/<id>/`）与标题键（`<id>:<标题>`）都用它，所以**改名不动一个文件**。
    /// 主命名空间是 [`MAIN_NS`]（"0" 占位）；`special` 是特例，标识与名字同为 "special"。
    #[serde(deserialize_with = "de_namespace_id")]
    pub id: String,
    /// 规范名（用户写在地址里的前缀）；主命名空间为空串（没有前缀）
    pub name: String,
    /// 别名（含其它语言）
    #[serde(default)]
    pub aliases: Vec<String>,
    /// 虚拟命名空间（Special / Media）不落存储
    pub storable: bool,
    /// 跨站链接的地址模板，`$1` 是页面名（如 `https://zh.wikipedia.org/wiki/$1`）。
    /// 有它的命名空间写成 `[[zhwiki:NASA]]` 时渲染成绿链，不再查本仓库有没有这一页。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub site: Option<String>,
}

impl Namespace {
    /// 跨站链接的地址：把页面名填进站点模板（`$1`）。
    ///
    /// 没有配站点就返回 `None`（那它就是个普通命名空间）。
    pub fn url_for(&self, page: &str) -> Option<String> {
        let template = self.site.as_ref()?;
        if page.is_empty() {
            return None;
        }
        Some(template.replace("$1", &encode_page(page)))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamespaceTable {
    pub items: Vec<Namespace>,
    /// 默认的跨站命名空间是否已经播过种。
    ///
    /// 有了这个标记，"默认就有"与"删得掉"才能同时成立：老仓库升级上来时补一次，
    /// 之后用户删掉它们就不会自己回来。老文件里没有这个字段 → 默认 false → 补一次。
    #[serde(default)]
    pub defaults_sown: bool,
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
        let mut table = Self {
            items: vec![
                // 主命名空间：没有前缀，用 "0" 占位
                Namespace {
                    id: MAIN_NS.to_string(),
                    name: String::new(),
                    aliases: Vec::new(),
                    storable: true,
                    site: None,
                },
                // 模板命名空间：用户自己写的模板（`.css` / `.html` 结尾的另有特判）
                Namespace {
                    id: TEMPLATE_NS.to_string(),
                    name: TEMPLATE_NS.to_string(),
                    aliases: Vec::new(),
                    storable: true,
                    site: None,
                },
                // 虚拟命名空间：`special:` 下的页面由程序提供，不落存储
                Namespace {
                    id: SPECIAL_NS.to_string(),
                    name: SPECIAL_NS.to_string(),
                    // 不需要 "Special" 之类的别名：查找本来就不区分大小写，
                    // 而"别名与规范名折过之后相同"正是查重要拦下的重复
                    aliases: Vec::new(),
                    storable: false,
                    site: None,
                },
            ],
            defaults_sown: false,
        };
        table.sow_defaults();
        table
    }

    /// 默认就有的两个**跨站**命名空间。
    ///
    /// 它们与其它命名空间没有任何特殊之处：可以改名、可以清空、可以删除 ——
    /// 删掉之后不会自己回来（见 [`NamespaceTable::sow_defaults`]）。
    pub fn defaults() -> [Namespace; 2] {
        [
            Namespace {
                id: "zhwiki".to_string(),
                name: "zhwiki".to_string(),
                aliases: vec!["中文维基百科".to_string()],
                storable: false,
                site: Some("https://zh.wikipedia.org/wiki/$1".to_string()),
            },
            Namespace {
                id: "qw".to_string(),
                name: "qw".to_string(),
                aliases: vec!["求闻百科".to_string()],
                storable: false,
                site: Some("https://www.qiuwenbaike.cn/wiki/$1".to_string()),
            },
        ]
    }

    /// 把默认的跨站命名空间**播一次种**，返回是否改动了这张表。
    ///
    /// 只在还没有播过的时候做（老仓库升级上来时补一次），做完就置位 —— 于是用户删掉它们
    /// 之后不会自己回来。名字或别名已被占用就跳过这一条：不能因为播种而制造重复。
    pub fn sow_defaults(&mut self) -> bool {
        if self.defaults_sown {
            return false;
        }
        for item in Self::defaults() {
            let taken = self.get(&item.id).is_some()
                || self.lookup(&item.name).is_some()
                || item.aliases.iter().any(|alias| self.lookup(alias).is_some());
            if !taken {
                self.items.push(item);
            }
        }
        self.defaults_sown = true;
        true
    }

    pub fn get(&self, id: &str) -> Option<&Namespace> {
        self.items.iter().find(|item| item.id == id)
    }

    /// 命名空间的显示名；`None` 表示这张表里没有它
    pub fn name_of(&self, id: &str) -> Option<&str> {
        self.get(id).map(|item| item.name.as_str())
    }

    /// 是不是虚拟命名空间（页面由程序提供，不落存储）
    pub fn is_virtual(&self, id: &str) -> bool {
        self.get(id).map(|item| !item.storable).unwrap_or(false)
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

    /// 这两个命名空间**删不掉、也不许占用它们的名字**：
    /// 主命名空间（`"0"`）与虚拟的 `special`。
    pub fn is_reserved(id: &str) -> bool {
        id == MAIN_NS || id == SPECIAL_NS || id == TEMPLATE_NS
    }

    /// 校验整张表：**名称与别名都不许重复**（大小写不敏感），别名也不许撞别人的规范名。
    ///
    /// 主命名空间的规范名是空串（它没有前缀），不参与查重；但它那个占位标识 `"0"`
    /// 也不该被谁当成名字用。
    pub fn validate(&self) -> Result<(), String> {
        // 折过的大小写/空白之后，所有"能被写出来的名字"必须互不相同
        let mut seen: Vec<(String, String)> = Vec::new();
        for item in &self.items {
            // 内建的那两个本来就叫 main / special：它们自己用保留名是应该的，
            // 只是不许别人占用 —— 所以照旧进 seen，但不因此报错
            let reserved = Self::is_reserved(&item.id);
            // 名字里的符号在建表时就该被拦下，但文件可能是手改的 —— 这里再兜一道
            if !item.name.is_empty() {
                Self::check_name(&item.name)?;
            }
            for alias in &item.aliases {
                Self::check_name(alias.trim())?;
            }
            let mut names: Vec<String> = Vec::new();
            if !item.name.is_empty() {
                names.push(item.name.clone());
            }
            names.extend(item.aliases.iter().cloned());

            for name in names {
                let folded = fold(&name);
                if folded.is_empty() {
                    return Err(format!("命名空间「{}」有空白名字", item.id));
                }
                if !reserved && Self::is_reserved(&folded) {
                    return Err(format!("「{name}」是保留名，不能用作命名空间名或别名"));
                }
                if let Some((owner, _)) = seen.iter().find(|(key, _)| *key == folded) {
                    return Err(format!(
                        "「{name}」与 {} 重名；名称与别名都不能重复",
                        owner
                    ));
                }
                seen.push((folded, name));
            }
        }
        Ok(())
    }

    /// 名字里不许有的符号：地址分隔符与文件系统敏感字符。
    ///
    /// `/` 尤其要拦：名字若进了路径就会变成目录层级。
    fn check_name(name: &str) -> Result<(), String> {
        for ch in name.chars() {
            if ch == '/' || ch == '\\' || ILLEGAL_CHARS.contains(&ch) {
                return Err(format!("命名空间名里不能有「{ch}」"));
            }
            if ch.is_control() {
                return Err("命名空间名里不能有控制字符".to_string());
            }
        }
        Ok(())
    }

    /// 下一个可用的标识。**生成的、与名字无关**，所以改名不必搬任何文件。
    fn next_id(&self) -> String {
        let mut n = 1;
        loop {
            let candidate = format!("ns{n}");
            if self.items.iter().all(|item| item.id != candidate) {
                return candidate;
            }
            n += 1;
        }
    }

    /// 加一个命名空间：名字用于显示与匹配，**标识另外生成**（两跳寻址）。
    pub fn add(&mut self, name: &str, aliases: Vec<String>, site: Option<String>) -> Result<(), String> {
        let name = name.trim().to_string();
        if name.is_empty() {
            return Err("命名空间名不能为空".to_string());
        }
        Self::check_name(&name)?;
        for alias in &aliases {
            Self::check_name(alias.trim())?;
        }
        if self.lookup(&name).is_some() {
            return Err(format!("「{name}」已经存在（或与某个别名相同）"));
        }

        let mut next = self.clone();
        next.items.push(Namespace {
            id: next.next_id(),
            // 配了站点地址 = 页面在别的站上，本仓库不存它的页面
            storable: site.is_none(),
            name,
            aliases,
            site,
        });
        next.validate()?;
        *self = next;
        Ok(())
    }

    /// 解析标题
    pub fn parse(&self, input: &str, capital_links: bool) -> Result<ParsedTitle, TitleError> {
        let normalized = normalize(input);
        if normalized.is_empty() {
            return Err(TitleError::Empty);
        }

        let (ns, rest) = match normalized.split_once(':') {
            Some((prefix, rest)) => match self.lookup(prefix) {
                // 命名空间命中：冒号归命名空间（虚拟命名空间不算合法**笔记标题**）
                Some(item) if item.storable => (item.id.clone(), rest.trim().to_string()),
                // **与 MediaWiki 明确不同**：冒号前缀不是已知命名空间时不再退回主命名空间，
                // 而是直接判非法。两条理由：冒号在文件系统路径里是麻烦字符；
                // 地址栏还要用「标题@版本」表达版本，需要一个干净的标题字符集。
                _ => return Err(TitleError::Illegal(':')),
            },
            None => (MAIN_NS.to_string(), normalized.clone()),
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

/// 模板命名空间里以 `.css` / `.html` 结尾的页面所用的语言标签。
///
/// 它们不是 markdown 文档，而是给 `::css` / `::html` 用的素材：阅读时当代码块显示，
/// 编辑器里用对应语言高亮。返回 `None` 表示按普通 markdown 处理。
///
/// 后缀不区分大小写（`样式.CSS` 也算）；只看**模板命名空间**，别处叫 `a.css` 的普通笔记
/// 不受影响。
pub fn code_template_language(ns: &str, name: &str) -> Option<&'static str> {
    if ns != TEMPLATE_NS {
        return None;
    }
    let lower = name.to_lowercase();
    if lower.ends_with(".css") {
        Some("css")
    } else if lower.ends_with(".html") {
        Some("html")
    } else {
        None
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedTitle {
    /// 命名空间标识（字符串；主命名空间是 [`MAIN_NS`]）
    pub ns: String,
    pub title: String,
}

impl ParsedTitle {
    /// 规范键：`<namespaceId>:<title>`。用数字 id 而非名字，命名空间改名不影响键。
    pub fn key(&self) -> String {
        format!("{}:{}", self.ns, self.title)
    }

    /// 显示标题：主命名空间不加前缀
    pub fn display(&self, table: &NamespaceTable) -> String {
        match table.name_of(&self.ns) {
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
    /// 跨站链接的外部地址；`Some` 表示这一条指向**别的站**，要画绿链并当外链打开
    pub url: Option<String>,
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
    /// 模板命名空间里的页面（名字 → 正文）。
    ///
    /// 预先把正文带进解析器，是为了让**渲染器**能按名字取素材：渲染发生在
    /// markdown-it 的回调里，那时没有仓库可查（见 `markdown::current_resolver`）。
    templates: Arc<HashMap<String, String>>,
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
            templates: Arc::new(HashMap::new()),
        }
    }

    /// 带上模板命名空间里的页面（[`crate::storage::Vault`] 构造时调用）
    pub fn with_templates(mut self, templates: Arc<HashMap<String, String>>) -> Self {
        self.templates = templates;
        self
    }

    /// 按名字取模板页的正文（`template:` 下的页面）
    pub fn template(&self, name: &str) -> Option<&str> {
        self.templates.get(name).map(String::as_str)
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

        // 跨站链接先判：前缀登记过站点地址时，它指向**别的站**，不在本仓库里找页面。
        // （这类命名空间是非存储的，普通解析本来也会把它挡掉 —— 所以必须抢在前面。）
        if let Some((prefix, page)) = body.split_once(':') {
            if let Some(item) = self.table.lookup(prefix) {
                if let Some(url) = item.url_for(page.trim()) {
                    return Some(Resolved {
                        key: item.id.clone(),
                        title: format!("{}:{}", item.name, page.trim()),
                        exists: true,
                        url: Some(url),
                    });
                }
            }
        }

        let parsed = if let Some(rest) = body.strip_prefix('/') {
            // 相对当前笔记的子页面：拼在完整标题后面，命名空间不变
            let from = self.from.as_ref()?;
            let suffix = normalize(rest);
            if suffix.is_empty() {
                return None;
            }
            ParsedTitle {
                ns: from.ns.clone(),
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
            url: None,
        })
    }
}

// ---------------------------------------------------------------- 小工具

/// 把页面名放进 URL 路径。只动确实会破坏 URL 的那几个 ASCII 字符；
/// 空格按 MediaWiki 的习惯折成 `_`；非 ASCII（中文标题）原样留着 —— 现代浏览器
/// 会自己转义，而提前转义反而容易弄错。
fn encode_page(page: &str) -> String {
    let mut out = String::with_capacity(page.len());
    for ch in page.chars() {
        match ch {
            ' ' => out.push('_'),
            '#' | '?' | '&' | '%' | '<' | '>' | '"' => {
                out.push_str(&format!("%{:02X}", ch as u32));
            }
            _ => out.push(ch),
        }
    }
    out
}

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

// ---------------------------------------------------------------- 路径转义
//
// 布局重写（按标题直查）是下一步，这几个函数届时会被 storage 用上

#[allow(dead_code)]
/// 在文件名里有问题、或本项目规则不允许出现在文件名里的字符
fn needs_escape(ch: char) -> bool {
    matches!(
        ch,
        '%' | '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'
    ) || ch.is_control()
}

/// 标题 → 文件名用的转义。
///
/// 只转义有问题的字符，其余（含 CJK）原样保留 —— 于是目录里看到的还是人能读的标题。
/// 转义可逆（`%XX` 是该字符 UTF-8 字节的大写十六进制），所以路径能反解回标题；
/// 这正是「按标题直查、不需要全局索引」的前提。
#[allow(dead_code)]
pub fn encode_for_path(title: &str) -> String {
    let mut out = String::with_capacity(title.len());

    for ch in title.chars() {
        if needs_escape(ch) {
            let mut buffer = [0u8; 4];
            for byte in ch.encode_utf8(&mut buffer).as_bytes() {
                out.push('%');
                out.push_str(&format!("{byte:02X}"));
            }
        } else {
            out.push(ch);
        }
    }

    // 整个名字都是点（`.` / `..`）时，在路径上会指向别处，转义掉第一个点
    if !out.is_empty() && out.chars().all(|ch| ch == '.') {
        out.replace_range(0..1, "%2E");
    }

    out
}

/// 文件名 → 标题。转义非法、或解出来的字节不是合法 UTF-8 时返回 `None`。
#[allow(dead_code)]
pub fn decode_from_path(name: &str) -> Option<String> {
    let mut bytes: Vec<u8> = Vec::with_capacity(name.len());
    let mut chars = name.chars();

    while let Some(ch) = chars.next() {
        if ch != '%' {
            let mut buffer = [0u8; 4];
            bytes.extend_from_slice(ch.encode_utf8(&mut buffer).as_bytes());
            continue;
        }

        let high = chars.next()?;
        let low = chars.next()?;
        let byte = u8::from_str_radix(&format!("{high}{low}"), 16).ok()?;
        bytes.push(byte);
    }

    String::from_utf8(bytes).ok()
}

#[cfg(test)]
mod tests {
    #[test]
    fn code_template_language_only_in_template_namespace() {
        assert_eq!(code_template_language(TEMPLATE_NS, "样式.css"), Some("css"));
        assert_eq!(code_template_language(TEMPLATE_NS, "片段.HTML"), Some("html"));
        assert_eq!(code_template_language(TEMPLATE_NS, "普通模板"), None);
        // 别处叫 .css 的普通笔记不受影响
        assert_eq!(code_template_language(MAIN_NS, "样式.css"), None);
    }

    use super::*;

    fn table() -> NamespaceTable {
        NamespaceTable::builtin()
    }

    /// 内建命名空间：主命名空间用 `"0"` 占位（它没有前缀），另有虚拟的 `special`。
    ///
    /// 标识是**字符串** —— 用户写在地址里的就是名字，没有"数字序号"这回事。
    #[test]
    fn builtin_namespaces_are_named_strings() {
        let table = table();

        let main = table.get(MAIN_NS).expect("主命名空间应当在表里");
        assert!(main.name.is_empty(), "主命名空间没有前缀");
        assert!(main.storable, "主命名空间是可存储的");

        let special = table.get(SPECIAL_NS).expect("special 应当在表里");
        assert_eq!(special.name, SPECIAL_NS, "规范名就是 special");
        assert!(!special.storable, "special 是虚拟命名空间：页面由程序提供");
        assert!(table.is_virtual(SPECIAL_NS));

        assert!(
            table.items.iter().all(|item| !item.id.is_empty()),
            "标识不该为空"
        );
    }

    #[test]
    fn main_namespace_has_no_prefix() {
        let parsed = table().parse("平陆运河", true).unwrap();
        assert_eq!(parsed.ns, MAIN_NS);
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
        assert_eq!(parsed.ns, MAIN_NS);
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
                ns: MAIN_NS.to_string(),
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
                ns: MAIN_NS.to_string(),
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

    #[test]
    fn path_escapes_are_reversible() {
        for title in [
            "平陆运河",
            "平陆运河/航道",
            "带:冒号",
            "带 空格",
            "带%百分号",
            "带\\反斜杠",
            "带\"引号\"",
            "带*星号?",
            "带<尖括号>|竖线",
            "末尾点.",
            "..",
            "...",
            "换\n行",
        ] {
            let encoded = encode_for_path(title);
            assert!(
                !encoded.contains('/'),
                "{title:?} 转义后不该还有斜杠：{encoded:?}"
            );
            assert_eq!(
                decode_from_path(&encoded).as_deref(),
                Some(title),
                "{title:?} 转义往返不一致：{encoded:?}"
            );
        }
    }

    #[test]
    fn readable_titles_stay_readable() {
        // CJK 与普通字符不转义，目录里要能直接看懂
        assert_eq!(encode_for_path("平陆运河"), "平陆运河");
        assert_eq!(encode_for_path("H1 Test"), "H1 Test");
        assert_eq!(encode_for_path("平陆运河/航道"), "平陆运河%2F航道");
    }

    #[test]
    fn dotted_names_are_made_safe() {
        assert_eq!(encode_for_path("."), "%2E");
        assert_eq!(encode_for_path(".."), "%2E.");
        assert_eq!(decode_from_path("%2E.").as_deref(), Some(".."));
    }

    #[test]
    fn bad_escapes_are_rejected() {
        assert!(decode_from_path("%ZZ").is_none());
        assert!(
            decode_from_path("%E4").is_none(),
            "残缺的多字节转义应当被拒"
        );
    }
}
