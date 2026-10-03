//! 这张表怎么读、怎么认一个名字。
//!
//! 全部是**纯内存**的操作：不碰仓库、不碰磁盘，所以能脱离 `Database` 单测。
//! 落盘那部分在兄弟模块 [`super::repository`]。
//!
//! 这里的三件事：
//!
//! - **读**（`get` / `lookup` / `name_of`）：标识、名字、别名三种写法都要认得；
//! - **改**（`add` / `rename` / `update` / `remove`）：改之前先克隆一份、自检不过整条丢弃，
//!   于是失败不会留下半成品；
//! - **自检**（`validate` + `check_name`）：名字与别名互不重复、不占保留名、词法能写进地址栏。
//!
//! 底下三个自由函数是配套的小工具：折叠匹配用的 `fold`、记账用的 `claim`、以及把页面名
//! 写进跨站 URL 的 `encode_page`。

use super::{
    Namespace, NamespaceTable, FILE_ID, FILE_NAME, HELP_ID, HELP_NAME, MAIN_ID, SPECIAL_ID,
    TEMPLATE_ID, TEMPLATE_NAME,
};
use crate::vault::title::ILLEGAL_CHARS;

impl NamespaceTable {
    /// 内建的那几个。
    pub fn builtin() -> Self {
        Self {
            defaults_sown: false,
            items: vec![
                Namespace {
                    id: MAIN_ID.to_string(),
                    name: String::new(),
                    aliases: Vec::new(),
                    storable: true,
                    site: None,
                },
                Namespace {
                    id: SPECIAL_ID.to_string(),
                    name: SPECIAL_ID.to_string(),
                    aliases: Vec::new(),
                    storable: false,
                    site: None,
                },
                Namespace {
                    id: FILE_ID.to_string(),
                    name: FILE_NAME.to_string(),
                    // 中文里也认「文件:桥.png」
                    aliases: vec!["文件".to_string()],
                    // 它是可存储的：文件就是页面，正文是字节
                    storable: true,
                    site: None,
                },
                Namespace {
                    id: HELP_ID.to_string(),
                    name: HELP_NAME.to_string(),
                    // 中文里也认「帮助:入门」
                    aliases: vec!["帮助".to_string()],
                    storable: false,
                    site: None,
                },
                Namespace {
                    id: TEMPLATE_ID.to_string(),
                    name: TEMPLATE_NAME.to_string(),
                    aliases: Vec::new(),
                    storable: true,
                    site: None,
                },
            ],
        }
    }

    pub fn get(&self, id: &str) -> Option<&Namespace> {
        // 空串是主命名空间的另一种写法：地址里没写前缀时就是它
        let id = if id.is_empty() { MAIN_ID } else { id };
        self.items.iter().find(|item| item.id == id)
    }

    /// 标识 → 规范名（显示用）。查不到就给标识本身：宁可在界面上露出个生名字，
    /// 也不要把一整篇笔记的名字变成空白。
    pub fn name_of(&self, id: &str) -> String {
        self.get(id)
            .map(|item| item.name.clone())
            .unwrap_or_else(|| id.to_string())
    }

    /// 前缀（规范名或别名，大小写与空白随便）→ 命名空间
    pub fn lookup(&self, prefix: &str) -> Option<&Namespace> {
        let wanted = fold(prefix);
        // 主命名空间没有前缀，但也认它自己的名字（空串与 "0"）——
        // 于是 `主:某页` 这种带前缀的写法落回主命名空间
        if wanted.is_empty() {
            return self.get(MAIN_ID);
        }
        self.items.iter().find(|item| {
            fold(&item.name) == wanted || item.aliases.iter().any(|alias| fold(alias) == wanted)
        })
    }

    /// 保留的：不能改名、不能删除。`special` 还多一条：不能配站点地址
    pub fn is_reserved(&self, id: &str) -> bool {
        id == MAIN_ID || id == SPECIAL_ID || id == HELP_ID || id == TEMPLATE_ID
    }

    /// 虚拟的：页面由程序提供，仓库里没有（`special`）
    pub fn is_virtual(&self, id: &str) -> bool {
        self.get(id).is_some_and(|item| !item.storable)
    }

    /// 把缺的**内建**命名空间补上（老仓库的 `namespaces.json` 里可能还没有它们）。
    ///
    /// 内建的那几个是程序认得的东西（`File:` 的文件、`Help:` 的帮助），仓库里缺了它们，
    /// 用户就会撞上"没有这个命名空间"—— 而这几个本来也不该由用户来建。
    /// 只按**标识**补，不碰任何已有的条目（名字、别名、跨站配置都原样）。
    /// 返回表有没有变（变了调用方要落盘）。
    pub fn ensure_builtins(&mut self) -> bool {
        let mut changed = false;
        for builtin in Self::builtin().items {
            if self.get(&builtin.id).is_none() {
                self.items.push(builtin);
                changed = true;
            }
        }
        changed
    }

    /// 种下两个默认的**跨站**命名空间（只在没播种过、且名字没被占用时）。
    ///
    /// 它们是给人**写链接**用的：`[[zhwiki:条目]]` 画成绿链，点了交给浏览器。
    /// 播种只发生一次，之后删掉就不会再回来 —— 默认值不该是"删不掉的东西"。
    /// 返回表有没有变（变了调用方要落盘）。
    pub fn sow_defaults(&mut self) -> bool {
        if self.defaults_sown {
            return false;
        }

        const DEFAULTS: [(&str, &[&str], &str); 2] = [
            (
                "zhwiki",
                &["中文维基百科"],
                "https://zh.wikipedia.org/wiki/$1",
            ),
            ("qw", &["求闻百科"], "https://www.qiuwenbaike.cn/wiki/$1"),
        ];

        for (name, aliases, site) in DEFAULTS {
            // 名字或别名被占了就跳过这一条：不跟人自己建的命名空间抢名字
            let taken = std::iter::once(name)
                .chain(aliases.iter().copied())
                .any(|candidate| self.lookup(candidate).is_some());
            if taken {
                continue;
            }
            let aliases: Vec<String> = aliases.iter().map(|alias| alias.to_string()).collect();
            self.items.push(Namespace {
                id: self.next_id(),
                name: name.to_string(),
                aliases,
                storable: false,
                site: Some(site.to_string()),
            });
        }

        self.defaults_sown = true;
        true
    }

    /// 下一个没被占用的标识：`ns1`、`ns2`……
    fn next_id(&self) -> String {
        for index in 1.. {
            let candidate = format!("ns{index}");
            if self.get(&candidate).is_none() {
                return candidate;
            }
        }
        unreachable!("标识总能找到下一个")
    }

    /// 整表自检：名字与别名互不重复、没有占保留名、没有空白名
    pub fn validate(&self) -> Result<(), String> {
        let mut seen: Vec<(String, String)> = Vec::new(); // (折叠后的名字, 谁占着)
        for item in &self.items {
            if item.name.trim().is_empty() {
                // 主命名空间就该是空的，别的不行
                if item.id != MAIN_ID {
                    return Err(format!("命名空间「{}」有空白名字", item.id));
                }
            } else {
                claim(&mut seen, &item.name, &item.id)?;
            }

            for alias in &item.aliases {
                if alias.trim().is_empty() {
                    return Err(format!("命名空间「{}」有空白别名", item.id));
                }
                claim(&mut seen, alias, &item.id)?;
            }
        }
        Ok(())
    }

    /// 名字 / 别名的词法：非空、没有保留字符 —— 它要能写进地址栏
    fn check_name(name: &str) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("命名空间名不能为空".to_string());
        }
        if let Some(ch) = name
            .chars()
            .find(|ch| ch.is_control() || ILLEGAL_CHARS.contains(ch) || *ch == '/')
        {
            return Err(format!("命名空间名里不能有「{ch}」"));
        }
        Ok(())
    }

    /// 新建一个。`site` 给了就是跨站命名空间（页面不落本仓库）
    pub fn add(
        &mut self,
        name: &str,
        aliases: Vec<String>,
        site: Option<String>,
    ) -> Result<(), String> {
        let name = name.trim().to_string();
        Self::check_name(&name)?;
        for alias in &aliases {
            Self::check_name(alias)?;
        }
        if self.lookup(&name).is_some() {
            return Err(format!("「{name}」已经存在（或与某个别名相同）"));
        }

        // 先克隆一份再改：自检不过就整条丢弃，不留下半成品
        let mut next = self.clone();
        next.items.push(Namespace {
            id: next.next_id(),
            name,
            aliases: normalized_aliases(aliases),
            storable: site.is_none(),
            site,
        });
        next.validate()?;
        *self = next;
        Ok(())
    }

    /// 改名：只动这一行，文件一个不挪
    pub fn rename(&mut self, id: &str, name: &str) -> Result<(), String> {
        if self.is_reserved(id) {
            return Err(format!(
                "「{}」是保留的命名空间，不能改名",
                self.name_of(id)
            ));
        }
        let name = name.trim().to_string();
        Self::check_name(&name)?;
        if self.get(id).is_none() {
            return Err(format!("命名空间「{id}」不存在"));
        }

        let mut next = self.clone();
        let item = next
            .items
            .iter_mut()
            .find(|item| item.id == id)
            .ok_or_else(|| format!("命名空间「{id}」不存在"))?;
        item.name = name;
        next.validate()?;
        *self = next;
        Ok(())
    }

    /// 改别名与站点地址。名称与标识都不动
    pub fn update(
        &mut self,
        id: &str,
        aliases: Vec<String>,
        site: Option<String>,
    ) -> Result<(), String> {
        for alias in &aliases {
            Self::check_name(alias)?;
        }
        if site.is_some() && self.is_reserved(id) {
            return Err(format!(
                "「{}」是保留的命名空间，不能配站点地址",
                self.name_of(id)
            ));
        }
        if self.get(id).is_none() {
            return Err(format!("命名空间「{id}」不存在"));
        }

        let mut next = self.clone();
        // 跨站与否由有没有站点地址决定；保留的（special）本来就不可存储，不动它
        let storable = if next.is_reserved(id) {
            next.get(id).map(|item| item.storable).unwrap_or(true)
        } else {
            site.is_none()
        };
        let item = next
            .items
            .iter_mut()
            .find(|item| item.id == id)
            .ok_or_else(|| format!("命名空间「{id}」不存在"))?;
        item.aliases = normalized_aliases(aliases);
        item.storable = storable;
        item.site = site;
        next.validate()?;
        *self = next;
        Ok(())
    }

    /// 从表里去掉。页面**不在这里处理** —— 调用方先把它们清空（进回收站）
    pub fn remove(&mut self, id: &str) -> Result<(), String> {
        if self.is_reserved(id) {
            return Err(format!(
                "「{}」是保留的命名空间，不能删除",
                self.name_of(id)
            ));
        }
        if self.get(id).is_none() {
            return Err(format!("命名空间「{id}」不存在"));
        }
        self.items.retain(|item| item.id != id);
        Ok(())
    }

    /// 表里的可存储命名空间标识（建目录时用）
    pub fn storable_ids(&self) -> Vec<String> {
        self.items
            .iter()
            .filter(|item| item.storable)
            .map(|item| item.id.clone())
            .collect()
    }
}

/// 匹配用的折叠：去空白 + 小写。别名与规范名都过它
fn fold(text: &str) -> String {
    text.trim().to_lowercase()
}

/// 记一个名字归属；撞了就报错
fn claim(seen: &mut Vec<(String, String)>, name: &str, owner: &str) -> Result<(), String> {
    let key = fold(name);
    if let Some((_, other)) = seen.iter().find(|(taken, _)| *taken == key) {
        if other != owner {
            return Err(format!(
                "「{name}」与「{}」重名；名称与别名都不能重复",
                other
            ));
        }
        return Err(format!(
            "「{name}」在同一命名空间里出现了两次（名字与别名不能相同）"
        ));
    }
    seen.push((key, owner.to_string()));
    Ok(())
}

/// 别名去掉空白与重复项
fn normalized_aliases(aliases: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for alias in aliases {
        let alias = alias.trim().to_string();
        if alias.is_empty() || out.iter().any(|kept| fold(kept) == fold(&alias)) {
            continue;
        }
        out.push(alias);
    }
    out
}

/// 跨站地址里的页面名：空格写成 `_`，几个会打断 URL 的字符转义
pub(super) fn encode_page(page: &str) -> String {
    let mut out = String::with_capacity(page.len());
    for ch in page.chars() {
        match ch {
            ' ' => out.push('_'),
            '#' | '?' | '&' | '%' | '<' | '>' | '"' => {
                let mut buffer = [0u8; 4];
                for byte in ch.encode_utf8(&mut buffer).bytes() {
                    out.push_str(&format!("%{byte:02X}"));
                }
            }
            _ => out.push(ch),
        }
    }
    out
}