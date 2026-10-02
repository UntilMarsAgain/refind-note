//! 用户帮助：仓库里 `help/` 目录下的那些 markdown。
//!
//! 整个目录**编译时**嵌进二进制（`include_dir`），运行时再照常渲染 —— 发布出去的
//! 程序不必带着那些文件，用户也改不了它们；而渲染走的还是**与笔记同一个渲染器**，
//! 所以帮助里的模板块、代码块、表格与笔记里一模一样。
//!
//! 两件事分开：
//!
//! - **菜单里列哪几页**：写死的三页（见 [`PAGES`]）—— 菜单是给人指路的，页数要克制，
//!   往 `help/` 里丢个新文件不该悄悄多出一项；
//! - **能打开哪些页**：`help/` 目录里**所有**的 `.md` —— 多出来的那些照样能被地址、
//!   `[[Help:…]]` 与 `refind://` 深链打开，只是暂时不在菜单里。
//!   （"菜单里没有"和"打不开"是两回事；后者是坏的。）
//!
//! 内容一律来自那个目录：**文件名就是页面名**（`首页.md` → `Help:首页`），
//! 不剥前缀、也不从正文另猜一个名字。

use include_dir::{include_dir, Dir};
use serde::Serialize;

use crate::vault::database::Database;

/// `help/` 整个目录：编译时烙进来
static HELP: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/help");

/// 帮助页所在的命名空间标识
pub const HELP_ID: &str = "help";

/// **帮助就这三页，顺序也定死**：首页 → 目录 → 语法速览。
///
/// 首页在前是因为它是入口；三页之间是"读什么"的顺序，不是文件名的排序
/// （按名字排的话，`首页` 会掉到最后 —— 它开头的字排得最靠后）。
///
/// 内容照旧来自 `help/` 目录（同名 `.md`），这张表只回答"菜单里列哪几页、什么次序"。
/// 目录里多出来的文件**不进菜单**，但**照样打得开**（见 [`find`]）；少了的按空页算
/// —— 名字写错、文件丢了都不该让菜单变形。测试盯着这几件事。
pub const PAGES: [&str; 3] = ["首页", "目录", "语法速览"];

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

/// 一页的正文（`help/<页面名>.md`；文件不在就是空串）
fn source_of(slug: &str) -> &'static str {
    HELP.get_file(format!("{slug}.md"))
        .and_then(|file| file.contents_utf8())
        .unwrap_or("")
}

/// **菜单里那三页**，按写死的顺序
fn menu_sources() -> Vec<(&'static str, &'static str)> {
    PAGES.iter().map(|slug| (*slug, source_of(slug))).collect()
}

/// `help/` 目录里**所有**的页，按文件名排
///
/// "能打开哪些页"归目录管：菜单只列那三页，但新丢进来的文件照样打得开 ——
/// 区别只是"菜单里没列它"，不是"打不开它"。
fn all_sources() -> Vec<(&'static str, &'static str)> {
    let mut pages: Vec<(&'static str, &'static str)> = HELP
        .files()
        .filter(|file| file.path().extension().is_some_and(|ext| ext == "md"))
        .filter_map(|file| {
            let stem = file.path().file_stem()?.to_str()?;
            let source = file.contents_utf8()?;
            Some((stem, source))
        })
        .collect();

    pages.sort_by(|a, b| a.0.cmp(b.0));
    pages
}

/// 全部帮助页（渲染在调用时发生，所以这一步要一个仓库 —— 渲染器在它身上）
pub fn pages(database: &Database) -> Vec<HelpPage> {
    menu_sources()
        .into_iter()
        .map(|(slug, source)| page_of(database, slug, source))
        .collect()
}

/// 找一页：认页面名（大小写与首尾空白宽松）
pub fn find(database: &Database, wanted: &str) -> Option<HelpPage> {
    let wanted = wanted.trim().to_lowercase();
    all_sources()
        .into_iter()
        .map(|(slug, source)| page_of(database, slug, source))
        .find(|page| page.slug.to_lowercase() == wanted)
}

/// 全部页面名（渲染 `[[Help:…]]` 时用：与笔记的键放进同一张表）
pub fn slugs() -> Vec<String> {
    all_sources()
        .into_iter()
        .map(|(slug, _)| slug.to_string())
        .collect()
}

/// 一页在不在（解析地址时用；不渲染，省一次开销）
pub fn exists(wanted: &str) -> bool {
    let wanted = wanted.trim().to_lowercase();
    all_sources()
        .iter()
        .any(|(slug, _)| slug.to_lowercase() == wanted)
}

/// 装配一页（**总有东西交出去**：正文坏了就交一个写着原因的页面）
fn page_of(database: &Database, slug: &str, source: &str) -> HelpPage {
    // 渲染时把这一页自己当"当前页"：`[[/子页]]` 之类按它的名字展开。
    //
    // 渲染不出来（正文里有坏模板之类）也**照样交出去**：菜单那三项是定死的，
    // 不该因为某一页有问题就少一项 —— 把原因写在页面里，比让它消失强。
    let html = database
        .render_html(source, &format!("{HELP_ID}:{slug}"))
        .unwrap_or_else(|error| {
            format!("<p class=\"help__problem\">这一页没能渲染出来：{error}</p>")
        });

    HelpPage {
        slug: slug.to_string(),
        display: format!("{}:{slug}", crate::vault::namespace::HELP_NAME),
        markdown: source.to_string(),
        html,
    }
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

    /// 菜单**就是这三页、这个顺序**（首页在最前，它是入口）
    #[test]
    fn the_help_menu_is_exactly_three_pages_in_order() {
        let database = scratch("fixed");

        let ordered: Vec<String> = pages(&database).into_iter().map(|page| page.slug).collect();
        assert_eq!(
            ordered,
            vec!["首页", "目录", "语法速览"],
            "菜单就是这三页、这个顺序"
        );

        cleanup(&database);
    }

    /// **菜单里没列的页，照样打得开** —— 往 `help/` 里丢一个新文件，它就该能被地址、
    /// `[[Help:…]]` 与深链打开（只是暂时不在菜单里）。
    ///
    /// 这里踩过一次：把"菜单那张表"同时用在了"能不能打开"上，于是除了那三页，
    /// 别的帮助页一律"没有这页帮助"（用户加的《开源协议》就这么打不开）。
    #[test]
    fn every_file_in_the_folder_is_openable_even_if_it_is_not_in_the_menu() {
        let database = scratch("reachable");

        for file in HELP.files() {
            let Some(stem) = file.path().file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            assert!(exists(stem), "{stem} 在 help/ 里，就该打得开");
            assert!(
                find(&database, stem).is_some(),
                "{stem} 在 help/ 里，就该找得到"
            );
            assert!(
                slugs().iter().any(|slug| slug == stem),
                "{stem} 应当在页面名单里（[[Help:…]] 要认得它）"
            );
        }

        cleanup(&database);
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
            // 空文件也是合法的一页（先占位、后填内容），所以只要求**有内容就渲染得出来**
            assert!(
                page.markdown.trim().is_empty() || page.html.contains('<'),
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
