//! 用户帮助：仓库里 `help/` 目录下的那些 markdown。
//!
//! 整个目录**编译时**嵌进二进制（`include_dir`），运行时再照常渲染 —— 发布出去的
//! 程序不必带着那些文件，用户也改不了它们；而渲染走的还是**与笔记同一个渲染器**，
//! 所以帮助里的模板块、代码块、表格与笔记里一模一样。
//!
//! 往里加一页 = 往 `help/` 里丢一个 `.md`：**文件名是什么，页面就叫什么**
//! （`首页.md` → `Help:首页`）。不剥前缀、也不从正文另猜一个名字 —— 磁盘上的叫法与
//! 界面上的叫法一致，才不会出现"这两个是不是同一页"的疑问。顺序就是文件名的顺序。

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
    /// 页面名（地址里写的那一段）—— **就是文件名去掉 `.md`**
    pub slug: String,
    /// 显示标题（`Help:首页`）
    pub display: String,
    /// 正文源码
    pub markdown: String,
    /// 渲染好的 HTML
    pub html: String,
}

/// 目录里的每一页，按文件名排序（顺序就是名字的顺序）
fn sources() -> Vec<(String, &'static str)> {
    let mut found: Vec<(String, &'static str)> = HELP
        .files()
        .filter(|file| file.path().extension().is_some_and(|ext| ext == "md"))
        .filter_map(|file| {
            // 文件名去掉 `.md` 就是页面名 —— 原样，不做任何加工
            let stem = file.path().file_stem()?.to_string_lossy().to_string();
            let source = file.contents_utf8()?;
            Some((stem, source))
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

/// 找一页：认页面名（大小写与首尾空白宽松）
pub fn find(database: &Database, wanted: &str) -> Option<HelpPage> {
    let wanted = wanted.trim().to_lowercase();
    sources()
        .into_iter()
        .filter_map(|(slug, source)| page_of(database, &slug, source))
        .find(|page| page.slug.to_lowercase() == wanted)
}

/// 全部页面名（渲染 `[[Help:…]]` 时用：与笔记的键放进同一张表）
pub fn slugs() -> Vec<String> {
    sources().into_iter().map(|(slug, _)| slug).collect()
}

/// 一页在不在（解析地址时用；不渲染，省一次开销）
pub fn exists(wanted: &str) -> bool {
    let wanted = wanted.trim().to_lowercase();
    sources()
        .iter()
        .any(|(slug, _)| slug.to_lowercase() == wanted)
}

/// 装配一页
fn page_of(database: &Database, slug: &str, source: &str) -> Option<HelpPage> {
    // 渲染时把这一页自己当"当前页"：`[[/子页]]` 之类按它的名字展开
    let html = database
        .render_html(source, &format!("{HELP_ID}:{slug}"))
        .ok()?;

    Some(HelpPage {
        slug: slug.to_string(),
        display: format!("{}:{slug}", crate::vault::namespace::HELP_NAME),
        markdown: source.to_string(),
        html,
    })
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

    /// **文件名就是页面名**：不剥前缀，也不从正文另起一个标题
    #[test]
    fn the_page_name_is_the_file_name_verbatim() {
        let database = scratch("naming");
        for page in pages(&database) {
            assert!(!page.slug.is_empty());
            assert_eq!(
                page.display,
                format!("{}:{}", crate::vault::namespace::HELP_NAME, page.slug),
                "界面上的名字应当就是文件名的原样"
            );
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

    /// 找得到：按页面名（大小写与空白宽松）；找不到就是找不到
    #[test]
    fn a_page_is_found_by_its_name() {
        let database = scratch("find");
        let all = pages(&database);
        if let Some(first) = all.first() {
            assert!(find(&database, &first.slug).is_some());
            assert!(find(&database, &format!("  {}  ", first.slug)).is_some());
            assert!(exists(&first.slug));
        }
        assert!(find(&database, "没有这一页帮助").is_none());
        assert!(!exists("没有这一页帮助"));
        cleanup(&database);
    }
}
