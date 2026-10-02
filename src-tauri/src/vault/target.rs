//   Refind Note is a note-taking software.
//   Copyright (C) 2026 Until Mars Again
//
//   This program is free software: you can redistribute it and/or modify
//   it under the terms of the GNU Affero General Public License as published by
//   the Free Software Foundation, either version 3 of the License, or
//   (at your option) any later version.
//
//   This program is distributed in the hope that it will be useful,
//   but WITHOUT ANY WARRANTY; without even the implied warranty of
//   MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
//   GNU Affero General Public License for more details.
//
//   You should have received a copy of the GNU Affero General Public License
//   along with this program.  If not, see <http://www.gnu.org/licenses/>.

//! 目标：**一个目标落到仓库上是什么**。
//!
//! 地址栏与正文里的 `[[链接]]` 问的是同一个问题 ——「`Help:入门` 是什么、在不在」。
//! 所以答案只在这里给一次：加一个命名空间、改一次"什么算一页"，都只动这一处；
//! 谁问都一样（地址解析、链接渲染，以后别的入口也走它）。
//!
//! 与 [`crate::vault::address`] 的分工：那边管**语法**（怎么写、状态与章节怎么读），
//! 这边管**语义**（写到的东西是什么）。`@edit`、`#段落` 只有地址栏会有，
//! 链接的目标就是"一页"，所以这里不带状态。

use std::collections::HashSet;
use std::sync::Arc;

use crate::features::help;
use crate::vault::address::SPECIAL_PAGES;
use crate::vault::database::Database;
use crate::vault::namespace::{NamespaceTable, HELP_ID, MAIN_ID, SPECIAL_ID};
use crate::vault::title::{self, ParsedTitle};

/// 现有页面的一张索引：谁在仓库里、谁随程序发布（帮助页）。
///
/// 渲染发生在 markdown 的回调里，那里没有仓库可查，所以由调用方在渲染前装配好
/// 交给渲染器 —— 但**装配这件事只有一处**（[`PageIndex::of`]），
/// 与地址解析问的是同一份答案。
#[derive(Debug, Clone, Default)]
pub struct PageIndex {
    keys: HashSet<String>,
}

impl PageIndex {
    /// 从仓库装配：笔记 + 帮助页
    pub fn of(database: &Database) -> Self {
        let table = database.namespaces();
        let mut keys: HashSet<String> = database
            .titles()
            .map(|titles| {
                titles
                    .notes
                    .values()
                    .filter_map(|display| title::parse(display, &table).ok())
                    .map(|parsed| parsed.key())
                    .collect()
            })
            .unwrap_or_default();

        // 帮助页不在仓库里，但同样是"一页"：它随程序发布
        for slug in help::slugs() {
            keys.insert(format!("{HELP_ID}:{slug}"));
        }

        Self { keys }
    }

    /// 从一堆规范键直接造一张索引（测试与"只想知道在不在"的场合用）
    pub fn of_keys(keys: impl IntoIterator<Item = String>) -> Self {
        Self {
            keys: keys.into_iter().collect(),
        }
    }

    /// 这一页在不在
    pub fn contains(&self, ns: &str, page: &str) -> bool {
        self.keys.contains(&format!("{ns}:{page}"))
    }

    /// 索引里有多少页（诊断与日志用）
    pub fn len(&self) -> usize {
        self.keys.len()
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }
}

/// 一个目标落到仓库上的结论
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// 一页：在仓库里、随程序发布（帮助），或者是虚拟命名空间里的特殊页面
    Page {
        /// 规范键：`<命名空间标识>:<页面名>`
        key: String,
        /// 显示标题（规范名 + 页面名）
        title: String,
        /// 目标是否存在（红链 / 蓝链就看它）
        exists: bool,
    },
    /// 跨站：目标是别人家的页面，只出得去
    CrossSite { title: String, url: String },
}

impl Target {
    pub fn key(&self) -> &str {
        match self {
            Target::Page { key, .. } => key,
            Target::CrossSite { .. } => "",
        }
    }

    pub fn title(&self) -> &str {
        match self {
            Target::Page { title, .. } | Target::CrossSite { title, .. } => title,
        }
    }

    pub fn exists(&self) -> bool {
        match self {
            Target::Page { exists, .. } => *exists,
            // 别人家的页面，"在不在"本仓库说了不算，所以一律当可达
            Target::CrossSite { .. } => true,
        }
    }

    pub fn url(&self) -> Option<&str> {
        match self {
            Target::Page { .. } => None,
            Target::CrossSite { url, .. } => Some(url),
        }
    }
}

/// 解析一个目标（链接的写法、地址里第一段都能进来）。`None` = 认不出来，留作字面文本。
///
/// 规则（与地址解析同一套）：
/// - 冒号前缀：跨站的出一趟门；帮助页、可存储命名空间、以及特殊页面都算"一页"；
/// - 前导斜杠：当前页的子页面；
/// - 其余：主命名空间里的页面。
pub fn resolve(
    target: &str,
    table: &NamespaceTable,
    index: &PageIndex,
    from: Option<&ParsedTitle>,
) -> Option<Target> {
    let trimmed = target.trim();
    if trimmed.is_empty() {
        return None;
    }

    // 前导 ':' 是「强制链接」语义；这里只把它脱掉，不影响目标
    let body = trimmed.strip_prefix(':').unwrap_or(trimmed).trim();

    if let Some((prefix, rest)) = body.split_once(':') {
        let found = table.lookup(prefix.trim())?;
        let page = title::check_page(&title::normalize(rest)).ok()?;

        // 跨站：地址由模板拼出来，页面本身不在本仓库
        if let Some(url) = found.url_for(&page) {
            return Some(Target::CrossSite {
                title: format!("{}:{page}", found.name),
                url,
            });
        }

        if found.id == SPECIAL_ID {
            // 特殊页面由程序提供，名字固定；认得出就一律可达
            let known = SPECIAL_PAGES
                .iter()
                .any(|known| known.eq_ignore_ascii_case(&page));
            return Some(Target::Page {
                key: format!("{SPECIAL_ID}:{}", page.to_lowercase()),
                title: format!("{}:{page}", found.name),
                exists: known,
            });
        }

        // 帮助页与可存储命名空间：在索引里查
        if !found.storable && found.id != HELP_ID {
            return None;
        }
        return Some(Target::Page {
            key: format!("{}:{page}", found.id),
            title: format!("{}:{page}", found.name),
            exists: index.contains(&found.id, &page),
        });
    }

    // 页面名的词法检查与地址栏**同一把尺子**（`check_page`）：
    // 标题不合法的目标不是地址，留作字面文本
    let (ns, page) = if let Some(rest) = body.strip_prefix('/') {
        // 相对当前页的子页面：拼在当前页面名后面，命名空间不变
        let from = from?;
        let suffix = title::check_page(&title::normalize(rest)).ok()?;
        (from.ns.clone(), format!("{}/{suffix}", from.page))
    } else {
        let name = title::check_page(&title::normalize(body)).ok()?;
        (MAIN_ID.to_string(), name)
    };

    Some(Target::Page {
        title: title::display_of_key(&format!("{ns}:{page}"), table),
        exists: index.contains(&ns, &page),
        key: format!("{ns}:{page}"),
    })
}

/// 渲染 `[[目标]]` 时用的解析器：把表与索引捆在一起，交给当次渲染。
///
/// 它自己不含规则 —— 规则在 [`resolve`] 那一处，与地址栏共用。
#[derive(Clone)]
pub struct Resolver {
    table: Arc<NamespaceTable>,
    index: Arc<PageIndex>,
    /// 当前页，用于展开 `/子页面`
    from: Option<ParsedTitle>,
}

impl std::fmt::Debug for Resolver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Resolver")
            .field("pages", &self.index.len())
            .field("from", &self.from)
            .finish()
    }
}

impl Resolver {
    pub fn new(
        table: Arc<NamespaceTable>,
        index: Arc<PageIndex>,
        from: Option<ParsedTitle>,
    ) -> Self {
        Self { table, index, from }
    }

    /// 解析一个链接目标；`None` 表示认不出来（留作字面文本）
    pub fn resolve(&self, target: &str) -> Option<Target> {
        resolve(target, &self.table, &self.index, self.from.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> NamespaceTable {
        NamespaceTable::builtin()
    }

    fn index(keys: &[&str]) -> PageIndex {
        PageIndex {
            keys: keys.iter().map(|key| key.to_string()).collect(),
        }
    }

    #[test]
    fn a_plain_name_lands_in_the_main_namespace() {
        let found = resolve("示例", &table(), &index(&["0:示例"]), None).unwrap();
        assert_eq!(found.key(), "0:示例");
        assert_eq!(found.title(), "示例");
        assert!(found.exists());
        assert!(found.url().is_none());
    }

    #[test]
    fn a_subpage_is_relative_to_the_current_page() {
        let table = table();
        let from = title::parse("现在这一页", &table).unwrap();
        let found = resolve("/子页", &table, &index(&["0:现在这一页/子页"]), Some(&from)).unwrap();
        assert_eq!(found.key(), "0:现在这一页/子页");
        assert!(found.exists());
    }

    #[test]
    fn a_cross_site_prefix_gives_a_url() {
        let mut table = table();
        table
            .add(
                "zhwiki",
                Vec::new(),
                Some("https://example.org/wiki/$1".to_string()),
            )
            .unwrap();

        let found = resolve("zhwiki:平陆运河", &table, &index(&[]), None).unwrap();
        assert_eq!(
            found.url(),
            Some("https://example.org/wiki/平陆运河"),
            "跨站链接只出得去"
        );
        assert!(found.exists(), "别人家的页面一律当可达");
    }

    #[test]
    fn help_pages_and_special_pages_are_pages_too() {
        let table = table();

        // 帮助页：随程序发布，在不在看索引
        let found = resolve("Help:入门", &table, &index(&["help:入门"]), None).unwrap();
        assert_eq!(found.key(), "help:入门");
        assert!(found.exists());

        // 特殊页面：名字固定，认得出就可达
        let settings = resolve("special:settings", &table, &index(&[]), None).unwrap();
        assert!(settings.exists());
        assert_eq!(settings.key(), "special:settings");

        let nonsense = resolve("special:没这页", &table, &index(&[]), None).unwrap();
        assert!(!nonsense.exists(), "没有的特殊页面是红链，不是可达");
    }

    #[test]
    fn unknown_prefixes_and_junk_stay_as_plain_text() {
        let table = table();
        assert!(resolve("foo:bar", &table, &index(&[]), None).is_none());
        assert!(resolve("   ", &table, &index(&[]), None).is_none());
        assert!(resolve("带#号的", &table, &index(&[]), None).is_none());
    }
}
