//! 用户帮助：仓库里 `help/` 目录下的那些 markdown。
//!
//! 整个目录**编译时**嵌进二进制（`include_dir`），运行时再照常渲染 —— 发布出去的
//! 程序不必带着那些文件，用户也改不了它们；而渲染走的还是**与笔记同一个渲染器**，
//! 所以帮助里的模板块、代码块、表格与笔记里一模一样。
//!
//! 往里加一页 = 往 `help/` 里丢一个 `.md`，文件名就是页面名；标题取正文第一个 `# ` 行。
//! 文件名前面可以带序号（`01 入门.md`）用来排序，序号不进页面名。

use include_dir::{include_dir, Dir};
use serde::Serialize;

use crate::vault::database::Database;

/// `help/` 整个目录：编译时烙进来
static HELP: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/help");

/// 帮助页所在的命名空间标识
pub const HELP_ID: &str = "help";

/// 一页帮助
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HelpPage {
    /// 页面名（地址里写的那一段）
    pub slug: String,
    /// 标题：正文第一个 `# ` 行，没有就用页面名
    pub title: String,
    /// 显示标题（`Help:入门`）
    pub display: String,
    /// 正文源码
    pub markdown: String,
    /// 渲染好的 HTML
    pub html: String,
}

/// 目录里的每一页，按文件名排序（序号就是拿来排顺序的）
fn sources() -> Vec<(String, &'static str)> {
    let mut found: Vec<(String, &'static str)> = HELP
        .files()
        .filter(|file| file.path().extension().is_some_and(|ext| ext == "md"))
        .filter_map(|file| {
            let stem = file.path().file_stem()?.to_string_lossy().to_string();
            let source = file.contents_utf8()?;
            Some((strip_order(&stem), source))
        })
        .collect();

    found.sort_by(|a, b| a.0.cmp(&b.0));
    found
}

/// 全部帮助页（渲染在调用时发生，所以这一步要一个仓库 —— 渲染器在它身上）
pub fn pages(database: &Database) -> Vec<HelpPage> {
    sources()
        .into_iter()
        .filter_map(|(slug, source)| page_of(database, &slug, source))
        .collect()
}

/// 找一页：认页面名，也认标题（大小写与首尾空白宽松）
pub fn find(database: &Database, wanted: &str) -> Option<HelpPage> {
    let wanted = wanted.trim().to_lowercase();
    sources()
        .into_iter()
        .filter_map(|(slug, source)| page_of(database, &slug, source))
        .find(|page| page.slug.to_lowercase() == wanted || page.title.to_lowercase() == wanted)
}

/// 全部页面名（渲染 `[[Help:…]]` 时用：与笔记的键放进同一张表）
pub fn slugs() -> Vec<String> {
    sources().into_iter().map(|(slug, _)| slug).collect()
}

/// 一页在不在（解析地址时用；不渲染，省一次开销）
pub fn exists(wanted: &str) -> bool {
    let wanted = wanted.trim().to_lowercase();
    sources().iter().any(|(slug, source)| {
        slug.to_lowercase() == wanted || title_of(source, slug).to_lowercase() == wanted
    })
}

/// 装配一页
fn page_of(database: &Database, slug: &str, source: &str) -> Option<HelpPage> {
    let title = title_of(source, slug);
    // 渲染时把这一页自己当"当前页"：`[[/子页]]` 之类按它的名字展开
    let html = database
        .render_html(source, &format!("{HELP_ID}:{slug}"))
        .ok()?;

    Some(HelpPage {
        slug: slug.to_string(),
        title,
        display: format!("{}:{slug}", crate::vault::namespace::HELP_NAME),
        markdown: source.to_string(),
        html,
    })
}

/// `01 入门` → `入门`；没有序号就原样返回
fn strip_order(stem: &str) -> String {
    let trimmed = stem.trim_start();
    let digits: String = trimmed
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect();
    if digits.is_empty() {
        return stem.to_string();
    }
    trimmed[digits.len()..]
        .trim_start_matches([' ', '-', '_', '.'])
        .trim()
        .to_string()
}

/// 标题：正文里第一个 `# ` 开头的行；没有就用页面名
fn title_of(source: &str, slug: &str) -> String {
    for line in source.lines() {
        if let Some(rest) = line.trim_end().strip_prefix("# ") {
            let title = rest.trim();
            if !title.is_empty() {
                return title.to_string();
            }
        }
    }
    slug.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> Database {
        let dir = std::env::temp_dir().join(format!(
            "refind-note-help-test-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let workspace = crate::storage::workspace::Workspace::open(dir).unwrap();
        Database::open(&workspace).unwrap()
    }

    fn cleanup(database: &Database) {
        if let Some(root) = database.root().parent() {
            let _ = std::fs::remove_dir_all(root);
        }
    }

    /// 序号不进页面名，标题取正文第一个 `# ` 行
    #[test]
    fn the_name_drops_the_order_and_the_title_comes_from_the_heading() {
        assert_eq!(strip_order("01 入门"), "入门");
        assert_eq!(strip_order("02-地址"), "地址");
        assert_eq!(strip_order("存储与加密"), "存储与加密");

        assert_eq!(title_of("# 入门\n\n正文\n", "入门"), "入门");
        assert_eq!(title_of("没有标题\n", "某页"), "某页");
        assert_eq!(
            title_of("#   两头有空格的标题  \n", "某页"),
            "两头有空格的标题"
        );
    }

    /// 嵌进来的每一页都要装配得出来：页面名不带序号、标题非空、正文渲染成 HTML
    #[test]
    fn every_embedded_page_assembles() {
        let database = scratch("pages");
        for page in pages(&database) {
            assert!(!page.slug.is_empty());
            assert!(
                !page
                    .slug
                    .chars()
                    .next()
                    .is_some_and(|ch| ch.is_ascii_digit()),
                "文件名前面的序号不该进页面名：{}",
                page.slug
            );
            assert_eq!(
                page.display,
                format!("{}:{}", crate::vault::namespace::HELP_NAME, page.slug)
            );
            assert!(!page.title.is_empty(), "{} 没有标题", page.slug);
            assert!(
                page.html.contains('<'),
                "{} 的正文应当渲染成 HTML",
                page.slug
            );
        }
        cleanup(&database);
    }

    /// 帮助页不进仓库：地址解析认得出它，找不到时说"没有这页帮助"
    #[test]
    fn a_help_address_resolves_to_a_help_page() {
        let database = scratch("resolve");
        let error = database.resolve_address("Help:没有这一页帮助").unwrap_err();
        assert!(error.contains("没有这页帮助"), "{error}");

        // 仓库里建不了帮助页：它不是笔记
        assert!(database.create("Help:想建一页").is_err());
        cleanup(&database);
    }

    /// 找得到：按页面名、按标题都认（大小写与空白宽松）；找不到就是找不到
    #[test]
    fn a_page_is_found_by_name_or_title() {
        let database = scratch("find");
        let all = pages(&database);
        if let Some(first) = all.first() {
            assert!(find(&database, &first.slug).is_some());
            assert!(find(&database, &format!("  {}  ", first.title)).is_some());
            assert!(exists(&first.slug));
        }
        assert!(find(&database, "没有这一页帮助").is_none());
        assert!(!exists("没有这一页帮助"));
        cleanup(&database);
    }
}
