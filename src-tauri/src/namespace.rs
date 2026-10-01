//! 命名空间：标题前缀那张表。
//!
//! 一篇笔记的身份是**两段**：命名空间 + 页面名。地址里的前缀（`帮助:入门`）说的就是
//! 前一段。这张表把「人写的前缀」与「内部标识」分开记：
//!
//! - **标识**（`id`）是稳定的：目录名（`objects/<标识>/`）、键（`<标识>:<页面名>`）都用它，
//!   所以**改名不动任何文件**；
//! - **名称 / 别名**是给人写的：地址栏、链接、列表里显示的都是它，大小写与首尾空白
//!   不影响匹配。
//!
//! 三个内建的：主命名空间（没有前缀）、`special`（虚拟，页面由程序提供）、
//! `template`（保留，模板与样式放这里）。其余的可以自己建：内容命名空间落在本仓库，
//! 也可以给一个站点地址模板，那就成了**跨站命名空间**（只用于链接，页面不在本仓库）。

use serde::{Deserialize, Serialize};

use crate::title::ILLEGAL_CHARS;

/// 主命名空间的标识（也是它目录的名字）。它没有前缀。
pub const MAIN_ID: &str = "0";
/// 虚拟命名空间 `special` 的标识：里面的页面由程序提供，不落仓库。
pub const SPECIAL_ID: &str = "special";
/// 保留命名空间 `template` 的标识。
pub const TEMPLATE_ID: &str = "template";

/// 一个命名空间。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Namespace {
    /// 标识：稳定键，磁盘目录与标题键都用它。改名不动它
    pub id: String,
    /// 规范名（地址里的前缀）；主命名空间是空串
    pub name: String,
    /// 别名：也认，回显时**用写下来的那一个**（不换算成规范名）
    #[serde(default)]
    pub aliases: Vec<String>,
    /// 页面是否落在本仓库。虚拟与跨站是 `false`
    pub storable: bool,
    /// 跨站地址模板，`$1` 是页面名。只有跨站命名空间才有
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub site: Option<String>,
}

impl Namespace {
    /// 跨站命名空间：页面不在本仓库，只有链接出得去
    pub fn is_cross_site(&self) -> bool {
        self.site.is_some()
    }

    /// 按模板拼出跨站地址；没有模板就没有地址
    pub fn url_for(&self, page: &str) -> Option<String> {
        self.site
            .as_ref()
            .map(|template| template.replace("$1", &encode_page(page)))
    }
}

/// 整张表。落盘在 `db/namespaces.json`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NamespaceTable {
    pub items: Vec<Namespace>,
}

impl Default for NamespaceTable {
    fn default() -> Self {
        Self::builtin()
    }
}

impl NamespaceTable {
    /// 三个内建的。
    pub fn builtin() -> Self {
        Self {
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
                    id: TEMPLATE_ID.to_string(),
                    name: TEMPLATE_ID.to_string(),
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
        id == MAIN_ID || id == SPECIAL_ID || id == TEMPLATE_ID
    }

    /// 虚拟的：页面由程序提供，仓库里没有（`special`）
    pub fn is_virtual(&self, id: &str) -> bool {
        self.get(id).is_some_and(|item| !item.storable)
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
fn encode_page(page: &str) -> String {
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

// ---------------------------------------------------------------- 仓库上的操作

use crate::database::Database;

impl Database {
    /// 整张表（命令直接把它交给界面）
    pub fn namespace_list(&self) -> Vec<Namespace> {
        self.namespaces().items
    }

    /// 新建一个命名空间，返回更新后的整张表。
    ///
    /// 表先落地、目录再补建：反过来的话，建目录成功而写表失败就会留下一个
    /// "有目录、没人认识"的命名空间。
    pub fn add_namespace(
        &self,
        name: &str,
        aliases: Vec<String>,
        site: Option<String>,
    ) -> Result<Vec<Namespace>, String> {
        let mut table = self.namespaces();
        table.add(name, aliases, site)?;
        self.save_namespaces(&table)?;
        self.ensure_namespace_dirs()?;
        Ok(table.items)
    }

    /// 改别名与站点地址
    pub fn update_namespace(
        &self,
        id: &str,
        aliases: Vec<String>,
        site: Option<String>,
    ) -> Result<Vec<Namespace>, String> {
        let mut table = self.namespaces();
        table.update(&namespace_id(&table, id), aliases, site)?;
        self.save_namespaces(&table)?;
        Ok(table.items)
    }

    /// 改名。文件一个不挪，只把两处名字改掉：表里那一行，和标题里写下来的前缀。
    pub fn rename_namespace(&self, id: &str, name: &str) -> Result<Vec<Namespace>, String> {
        let mut table = self.namespaces();
        let id = namespace_id(&table, id);
        let old = table.name_of(&id);
        table.rename(&id, name)?;
        let new = table.name_of(&id);
        self.save_namespaces(&table)?;

        // 显示标题里存着旧前缀：不加这一道，那些笔记就会"表里改了、标题里还写着旧的"
        let mut titles = self.titles()?;
        rename_in_titles(&mut titles.notes, &old, &new);
        rename_in_titles(&mut titles.trashed, &old, &new);
        self.save_titles(&titles)?;

        Ok(table.items)
    }

    /// 清空：把里面的页面**全部移进回收站**（不是抹掉），命名空间本身留着
    pub fn empty_namespace(&self, id: &str) -> Result<usize, String> {
        let table = self.namespaces();
        let id = namespace_id(&table, id);
        if table.get(&id).is_none() {
            return Err(format!("命名空间「{id}」不存在"));
        }

        // 先列出它名下的笔记（显示标题），再一个个走正常的删除路径
        let titles = self.titles()?;
        let mut doomed: Vec<String> = Vec::new();
        for display in titles.notes.values() {
            if let Ok(parsed) = crate::title::parse(display, &table) {
                if parsed.ns == id {
                    doomed.push(display.clone());
                }
            }
        }

        for title in &doomed {
            self.delete(title)?;
        }
        Ok(doomed.len())
    }

    /// 删除命名空间：先清空（页面进回收站），再从表里去掉那一行
    pub fn delete_namespace(&self, id: &str) -> Result<usize, String> {
        let mut table = self.namespaces();
        let id = namespace_id(&table, id);
        let removed = self.empty_namespace(&id)?;
        table.remove(&id)?;
        self.save_namespaces(&table)?;
        Ok(removed)
    }
}

/// 命令里的 `key` 可以是标识、规范名或别名 —— 统一换算成标识
fn namespace_id(table: &NamespaceTable, key: &str) -> String {
    if table.get(key).is_some() {
        return key.to_string();
    }
    table
        .lookup(key)
        .map(|item| item.id.clone())
        .unwrap_or_else(|| key.to_string())
}

/// 把标题表里写着旧前缀的显示标题换成新前缀
fn rename_in_titles(titles: &mut std::collections::BTreeMap<String, String>, old: &str, new: &str) {
    for display in titles.values_mut() {
        let Some((prefix, page)) = display.split_once(':') else {
            continue;
        };
        if prefix != old {
            continue;
        }
        // 主命名空间没有前缀，改回它时要把冒号也去掉
        *display = if new.is_empty() {
            page.to_string()
        } else {
            format!("{new}:{page}")
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::address;

    fn scratch(name: &str) -> Database {
        let dir =
            std::env::temp_dir().join(format!("refind-note-ns-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let workspace = crate::workspace::Workspace::open(dir).unwrap();
        Database::open(&workspace).unwrap()
    }

    fn cleanup(database: &Database) {
        if let Some(root) = database.root().parent() {
            let _ = std::fs::remove_dir_all(root);
        }
    }

    #[test]
    fn the_three_builtins_are_there_from_the_start() {
        let table = NamespaceTable::builtin();
        assert_eq!(table.name_of(MAIN_ID), "");
        assert_eq!(table.name_of(SPECIAL_ID), "special");
        assert!(table.get(TEMPLATE_ID).is_some());
        assert!(table.is_virtual(SPECIAL_ID));
        assert!(!table.is_virtual(TEMPLATE_ID));
    }

    #[test]
    fn aliases_are_matched_loosely_but_displayed_canonically() {
        let mut table = NamespaceTable::builtin();
        table.add("help", vec!["帮助".to_string()], None).unwrap();

        let by_alias = table.lookup(" 帮助 ").expect("别名应当认得出");
        assert_eq!(by_alias.id, "ns1");
        assert_eq!(
            table.lookup("HELP").map(|item| item.id.as_str()),
            Some("ns1")
        );

        // 回显用规范名：别名只是"也认"，不是它的名字
        let parsed = crate::title::parse("帮助:入门", &table).unwrap();
        assert_eq!(parsed.key(), "ns1:入门");
        assert_eq!(parsed.display(&table), "help:入门");
    }

    #[test]
    fn names_and_aliases_cannot_collide() {
        let mut table = NamespaceTable::builtin();
        table.add("help", Vec::new(), None).unwrap();
        assert!(table
            .add("Help", Vec::new(), None)
            .unwrap_err()
            .contains("已经存在"));
        assert!(table
            .add("other", vec!["帮助2".to_string(), "help".to_string()], None)
            .unwrap_err()
            .contains("重名"));
        // 保留名不能占用
        assert!(table
            .add("special", Vec::new(), None)
            .unwrap_err()
            .contains("已经存在"));
    }

    #[test]
    fn reserved_namespaces_cannot_be_renamed_or_removed() {
        let mut table = NamespaceTable::builtin();
        assert!(table
            .rename(SPECIAL_ID, "别的")
            .unwrap_err()
            .contains("不能改名"));
        assert!(table.remove(MAIN_ID).unwrap_err().contains("不能删除"));
        // 但可以给它们配别名
        table.update(MAIN_ID, vec!["主".to_string()], None).unwrap();
        assert!(table.lookup("主").is_some());
    }

    #[test]
    fn a_namespaced_note_is_addressable_and_keeps_its_namespace() {
        let database = scratch("addressable");

        let table = database.namespaces();
        let created = database
            .add_namespace("help", vec!["帮助".to_string()], None)
            .unwrap();
        assert_eq!(created.len(), 4);

        let title = database.create("help:入门").unwrap();
        assert_eq!(title, "help:入门");

        // 用别名访问：回显保留别名，落到仓库上仍然是同一篇
        let resolved = database
            .resolve_address("帮助:入门")
            .unwrap()
            .expect("不是空输入");
        assert_eq!(resolved.canonical, "帮助:入门");
        assert_eq!(
            resolved.outcome,
            crate::resolve::Outcome::Note {
                title: "help:入门".to_string()
            }
        );

        // 不带前缀的名字落在主命名空间，不会撞上 help:入门
        let plain = database
            .resolve_address("入门")
            .unwrap()
            .expect("不是空输入");
        assert_eq!(
            plain.outcome,
            crate::resolve::Outcome::Missing {
                title: "入门".to_string()
            }
        );

        cleanup(&database);
    }

    #[test]
    fn rename_moves_no_files_but_rewrites_the_titles() {
        let database = scratch("rename");
        database.add_namespace("help", Vec::new(), None).unwrap();
        database.create("help:入门").unwrap();

        let id = database.id_of("help:入门").expect("刚建的就该找得到");
        let before = database.log_path(&id);

        database.rename_namespace("ns1", "手册").unwrap();

        // 文件一个不挪，键也不变 —— 变的只有显示标题
        assert_eq!(database.log_path(&id), before);
        assert!(before.is_file());
        assert!(database.exists("手册:入门"));
        assert_eq!(database.id_of("手册:入门").as_deref(), Some(id.as_str()));
        // 旧名字不认了
        assert!(!database.exists("help:入门"));
        assert_eq!(database.display_of("手册:入门").unwrap(), "手册:入门");

        cleanup(&database);
    }

    #[test]
    fn emptying_a_namespace_moves_its_notes_to_the_trash() {
        let database = scratch("empty");
        database.add_namespace("help", Vec::new(), None).unwrap();
        database.create("help:一").unwrap();
        database.create("help:二").unwrap();
        database.create("留下来的").unwrap();

        let moved = database.empty_namespace("help").unwrap();
        assert_eq!(moved, 2);

        // 命名空间还在，里面的页面进了回收站
        assert!(database.namespaces().get("ns1").is_some());
        assert!(!database.exists("help:一"));
        assert!(database.exists("留下来的"));

        // 删除本身会把名字挪进 trashed
        let titles = database.titles().unwrap();
        assert_eq!(
            titles
                .trashed
                .values()
                .filter(|name| name.starts_with("help:"))
                .count(),
            2
        );

        cleanup(&database);
    }

    #[test]
    fn deleting_a_namespace_empties_it_first() {
        let database = scratch("delete-ns");
        database.add_namespace("help", Vec::new(), None).unwrap();
        database.create("help:入门").unwrap();

        let moved = database.delete_namespace("help").unwrap();
        assert_eq!(moved, 1);
        assert!(database.namespaces().get("ns1").is_none());
        assert!(!database.exists("help:入门"));

        cleanup(&database);
    }

    #[test]
    fn a_cross_site_namespace_holds_no_pages() {
        let database = scratch("cross-site");
        database
            .add_namespace(
                "zhwiki",
                vec!["中文维基".to_string()],
                Some("https://example.org/wiki/$1".to_string()),
            )
            .unwrap();

        let table = database.namespaces();
        let found = table.lookup("中文维基").unwrap();
        assert!(found.is_cross_site());
        assert!(!found.storable);
        assert!(database.create("zhwiki:平陆运河").is_err());

        // 地址栏里敲它：这一页不在本仓库，说清楚比"找不到"有用
        assert!(database
            .resolve_address("zhwiki:平陆运河")
            .unwrap_err()
            .contains("跨站"));
        assert!(address::parse("zhwiki:平陆运河", &table)
            .unwrap_err()
            .contains("跨站"));

        cleanup(&database);
    }
}
