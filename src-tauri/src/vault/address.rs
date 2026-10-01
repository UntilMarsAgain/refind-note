//! 地址：`命名空间:页面名称@浏览状态#段落` 的解析与规范拼写。
//!
//! 这一层只做**语法**：前缀、状态词、章节怎么读，页面名怎么规整、哪些字符不合法。
//! 页面在不在、是哪一版 —— 一切要看仓库内容的问题都不归这里管。
//!
//! 解析产物 `Address` 是地址的**身份**：稳定、可比较、可持久化；`canonical`
//! 是它对应的规范串（`compose` 的结果），地址栏回显、历史、剪贴板都用它 ——
//! 前端不自己拼地址字符串。

use serde::Serialize;

use crate::vault::namespace::{NamespaceTable, HELP_ID, SPECIAL_ID};
use crate::vault::title;

/// 现有的特殊页面。不在这里面的 `special:` 地址直接报「没有这个特殊页面」。
pub const SPECIAL_PAGES: [&str; 11] = [
    "newtab", "settings", "all", "random", "debug", "trash", "gc", "files", "changes", "history",
    "keys",
];

/// 命名空间部分：`id` 是它的身份，`spelling` 是回显时用的拼写。
///
/// 主命名空间没有前缀，两者都是空串。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NamespaceRef {
    pub id: String,
    pub spelling: String,
}

/// 浏览状态：`@` 后面那一段。
///
/// `reference` 收下的只是 **token**：它是版本号还是别的引用形式，由仓库那一层解释。
/// `None` 表示这一种状态不指版本 —— 阅读 = 最新版，解锁 = 最新版。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Mode {
    /// 阅读：`None` 是最新版（规范串里缩写掉 `@view`），`Some` 是 `@view-<token>`
    View {
        #[serde(rename = "ref")]
        reference: Option<String>,
    },
    Edit,
    History,
    Delete,
    /// 回退：必须指明版本（`@rollback-<token>`）
    Rollback {
        #[serde(rename = "ref")]
        reference: String,
    },
    /// 等待口令：`@unlock`（最新版）或 `@unlock-<token>`
    Unlock {
        #[serde(rename = "ref")]
        reference: Option<String>,
    },
    /// `@no-command`：这一页是指令页，但**不跟跳**，照原文看
    NoCommand,
}

/// 一个地址：`命名空间:页面名称@浏览状态#段落`。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Address {
    pub namespace: NamespaceRef,
    /// 页面名。规整过：`_` 视作空格、空白折叠、英文首字母大写
    pub page: String,
    pub mode: Mode,
    /// 章节（段落）。空串 = 没写
    pub section: String,
}

/// 一次解析的产物：地址本身 + 它的规范串。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ParsedAddress {
    pub address: Address,
    /// 规范串：地址栏回显、历史都用它
    pub canonical: String,
}

/// 解析地址栏那一行：空输入不是地址（`None`）；语法有问题时，错误里是一句给人看的话。
///
/// 要那张命名空间表：`帮助:入门` 说的到底是哪个命名空间，是表说了算 ——
/// 所以"语法"这一层也得看得见它。
pub fn parse(input: &str, table: &NamespaceTable) -> Result<Option<ParsedAddress>, String> {
    let raw = input.trim();
    if raw.is_empty() {
        return Ok(None);
    }

    let AddressParts {
        name,
        state,
        section,
    } = split_address(raw);
    let name = title::normalize(&name);

    // 冒号只可能属于命名空间前缀。认不出来就报错，**不回退主命名空间** ——
    // MediaWiki 那套（未知前缀一律当主命名空间的标题）到这里是打错的字，
    // 静默收下只会让人以为"这篇笔记没了"。
    if let Some((prefix, rest)) = name.split_once(':') {
        let prefix = prefix.trim();
        if prefix.is_empty() {
            return Err(title::reject_namespace(prefix, table));
        }
        let Some(found) = table.lookup(prefix) else {
            return Err(title::reject_namespace(prefix, table));
        };

        if found.storable {
            let address = Address {
                namespace: NamespaceRef {
                    id: found.id.clone(),
                    // 回显用**写下来的那个拼写**：用别名访问就用别名还回去
                    spelling: prefix.to_string(),
                },
                page: title::check_page(rest.trim())?,
                mode: mode_of(state.as_deref().unwrap_or(""))?,
                section: section.unwrap_or_default(),
            };
            let canonical = compose(&address);
            return Ok(Some(ParsedAddress { address, canonical }));
        }

        if found.id == SPECIAL_ID {
            return special(rest.trim(), section, table).map(Some);
        }
        // 跨站命名空间：页面在别人家，但它**是个地址** —— 写成地址就得解析得出来，
        // 落到仓库上会得到"交给浏览器打开"那个结论（见 resolve）
        if found.is_cross_site() {
            let address = Address {
                namespace: NamespaceRef {
                    id: found.id.clone(),
                    spelling: prefix.to_string(),
                },
                page: title::check_page(rest.trim())?,
                mode: mode_of(state.as_deref().unwrap_or(""))?,
                section: section.unwrap_or_default(),
            };
            let canonical = compose(&address);
            return Ok(Some(ParsedAddress { address, canonical }));
        }
        if found.id == HELP_ID {
            // 帮助页：页面名在**编译进来的那张表**里，这里只做词法检查，
            // 在不在由 resolve 那一层回答（它才拿得到仓库）
            let address = Address {
                namespace: NamespaceRef {
                    id: HELP_ID.to_string(),
                    spelling: prefix.to_string(),
                },
                page: title::check_page(rest.trim())?,
                mode: mode_of(state.as_deref().unwrap_or(""))?,
                section: section.unwrap_or_default(),
            };
            let canonical = compose(&address);
            return Ok(Some(ParsedAddress { address, canonical }));
        }
        // 剩下的只可能是"不可存储、又不是特殊/帮助/跨站"的命名空间 ——
        // 内建的三个都不落这一支，留着是给以后新加的命名空间一个说得清的错
        return Err(format!("「{}」不是能写成地址的命名空间", found.name));
    }

    let address = Address {
        namespace: NamespaceRef {
            id: crate::vault::namespace::MAIN_ID.to_string(),
            spelling: String::new(),
        },
        page: title::check_page(&name)?,
        mode: mode_of(state.as_deref().unwrap_or(""))?,
        section: section.unwrap_or_default(),
    };
    let canonical = compose(&address);
    Ok(Some(ParsedAddress { address, canonical }))
}

/// `special:` 前缀：页面标识按小写查注册表，规范串里写成 `Special:Settings`。
///
/// 特殊页面没有版本、编辑、删除这类状态：`@` 后面写了什么一律裁掉，章节保留 ——
/// 地址栏回显出来的规范形状（少了那一段）本身就是提示。
fn special(
    page: &str,
    section: Option<String>,
    table: &NamespaceTable,
) -> Result<ParsedAddress, String> {
    let id = page.to_lowercase();
    if id.is_empty() {
        return Err("special: 后面要写页面名，例如 special:newtab".to_string());
    }
    if !SPECIAL_PAGES.contains(&id.as_str()) {
        return Err(format!(
            "没有这个特殊页面：special:{id}（现有：{}）",
            SPECIAL_PAGES.join("、")
        ));
    }

    let address = Address {
        namespace: NamespaceRef {
            id: SPECIAL_ID.to_string(),
            spelling: title::capitalize_first(&table.name_of(SPECIAL_ID)),
        },
        page: title::capitalize_first(&id),
        mode: Mode::View { reference: None },
        section: section.unwrap_or_default(),
    };
    let canonical = compose(&address);
    Ok(ParsedAddress { address, canonical })
}

/// `@` 后面那一整段 → `Mode`。
///
/// 状态词不分大小写；`-` 后面是 **token**，原样收下（`@View-ABC` 的 token 是 `ABC`）。
fn mode_of(state: &str) -> Result<Mode, String> {
    let raw = state.trim();
    if raw.is_empty() {
        return Ok(Mode::View { reference: None });
    }

    // `no-command` 自己带一个连字符，必须在拆 `词-参数` 之前认出来
    if raw.eq_ignore_ascii_case("no-command") {
        return Ok(Mode::NoCommand);
    }

    let (keyword, reference) = match raw.split_once('-') {
        Some((keyword, reference)) => (keyword, Some(reference.trim())),
        None => (raw, None),
    };

    match (keyword.to_lowercase().as_str(), reference) {
        ("view", None) => Ok(Mode::View { reference: None }),
        // `@view-` 是漏写的版本：它与 `@view`（最新版）不是一回事
        ("view", Some("")) => Err("「@view-」后面要写版本".to_string()),
        ("view", Some(reference)) => Ok(Mode::View {
            reference: Some(reference.to_string()),
        }),
        ("edit", None) => Ok(Mode::Edit),
        ("history", None) => Ok(Mode::History),
        ("delete", None) => Ok(Mode::Delete),
        ("unlock", None) => Ok(Mode::Unlock { reference: None }),
        ("unlock", Some("")) => Err("「@unlock-」后面要写版本".to_string()),
        ("unlock", Some(reference)) => Ok(Mode::Unlock {
            reference: Some(reference.to_string()),
        }),
        // 回退到最新版没有意义，所以必须指明版本
        ("rollback", None | Some("")) => Err("回退要指明版本：@rollback-<版本>".to_string()),
        ("rollback", Some(reference)) => Ok(Mode::Rollback {
            reference: reference.to_string(),
        }),
        // 认不出来的状态要报错，不能静默裁掉 —— 否则打错的 `@edti` 会悄悄变成阅读页
        _ => Err(format!("不认识这个状态：@{raw}")),
    }
}

/// 拼规范串：`[命名空间:]页面名称[@状态][#段落]`，状态排在段落前面。
///
/// 阅读最新版（`Mode::View` 不带 token）缩写掉 `@view` —— 裸名称就是它的规范形状。
pub(crate) fn compose(address: &Address) -> String {
    let mut out = String::new();
    if !address.namespace.spelling.is_empty() {
        out.push_str(&address.namespace.spelling);
        out.push(':');
    }
    out.push_str(&address.page);
    if let Some(state) = state_of(&address.mode) {
        out.push('@');
        out.push_str(&state);
    }
    if !address.section.is_empty() {
        out.push('#');
        out.push_str(&address.section);
    }
    out
}

/// 状态在规范串里的写法；`None` = 缩写掉。
fn state_of(mode: &Mode) -> Option<String> {
    match mode {
        Mode::View { reference } => reference.as_ref().map(|token| format!("view-{token}")),
        Mode::Edit => Some("edit".to_string()),
        Mode::History => Some("history".to_string()),
        Mode::Delete => Some("delete".to_string()),
        Mode::Rollback { reference } => Some(format!("rollback-{reference}")),
        Mode::Unlock { reference } => Some(match reference {
            Some(token) => format!("unlock-{token}"),
            None => "unlock".to_string(),
        }),
        Mode::NoCommand => Some("no-command".to_string()),
    }
}

/// 拆开后的三个成分。
struct AddressParts {
    name: String,
    state: Option<String>,
    section: Option<String>,
}

/// 把 `名称[@状态][#段落]` 拆成三部分。
///
/// 名称之外的成分各以 `@` / `#` 开头，所以**书写顺序自由**（`名称#段落@状态` 也认得）；
/// 同名成分后者覆盖前者（`a#x#y` 的段落是 `y`），空成分算没写。
fn split_address(raw: &str) -> AddressParts {
    let name_end = raw
        .char_indices()
        .find(|(_, ch)| matches!(ch, '@' | '#'))
        .map(|(index, _)| index)
        .unwrap_or(raw.len());
    let name = raw[..name_end].trim().to_string();
    let rest = &raw[name_end..];

    let mut state: Option<String> = None;
    let mut section: Option<String> = None;
    let mut cursor = 0;
    while cursor < rest.len() {
        let marker = rest[cursor..].chars().next().expect("非空");
        let from = cursor + marker.len_utf8();
        let to = rest[from..]
            .find(['@', '#'])
            .map(|offset| from + offset)
            .unwrap_or(rest.len());
        let value = rest[from..to].trim().to_string();

        match marker {
            '@' => state = Some(value),
            '#' => section = Some(value),
            _ => {}
        }
        cursor = to;
    }

    AddressParts {
        name,
        state: state.filter(|value| !value.is_empty()),
        section: section.filter(|value| !value.is_empty()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试用的命名空间表：内建的那三个
    fn table() -> NamespaceTable {
        NamespaceTable::builtin()
    }

    /// 解析一个必定是地址的输入
    fn parsed(input: &str) -> ParsedAddress {
        parse(input, &table())
            .unwrap_or_else(|reason| panic!("{input:?} 本该解析成功，却报了：{reason}"))
            .unwrap_or_else(|| panic!("{input:?} 本该是一个地址，却是空输入"))
    }

    /// 规范串 —— 地址栏该回显的那一串
    fn canonical(input: &str) -> String {
        parsed(input).canonical
    }

    /// 取报错理由（成功则 panic）
    fn reason(input: &str) -> String {
        match parse(input, &table()) {
            Ok(value) => panic!("{input:?} 本该报错，却解析成了 {value:?}"),
            Err(reason) => reason,
        }
    }

    #[test]
    fn empty_input_is_not_an_address() {
        assert_eq!(parse("", &table()), Ok(None));
        assert_eq!(parse("   ", &table()), Ok(None));
    }

    /// 帮助页的地址：前缀认规范名，也认中文别名
    #[test]
    fn a_help_address_is_parsed() {
        let it = parsed("Help:入门");
        assert_eq!(it.address.namespace.id, "help");
        assert_eq!(it.address.namespace.spelling, "Help");
        assert_eq!(it.canonical, "Help:入门");
        assert_eq!(parsed("帮助:入门").address.namespace.id, "help");
        // `@edit` 在帮助里是"看源码"，所以状态照样收下（由 resolve 决定怎么用）
        assert_eq!(parsed("Help:入门@edit").address.mode, Mode::Edit);
    }

    #[test]
    fn a_plain_title_has_a_canonical_address() {
        let it = parsed("示例笔记");
        assert_eq!(it.canonical, "示例笔记");
        assert_eq!(it.address.page, "示例笔记");
        assert_eq!(it.address.namespace.id, crate::vault::namespace::MAIN_ID);
        assert_eq!(it.address.namespace.spelling, "");
        assert_eq!(it.address.mode, Mode::View { reference: None });
        assert_eq!(it.address.section, "");
    }

    #[test]
    fn english_names_are_capitalized() {
        assert_eq!(canonical("example"), "Example");
        assert_eq!(canonical("hello world"), "Hello world");
        // 中文不受影响（CJK 的 to_uppercase 是它自己）
        assert_eq!(canonical("示例笔记"), "示例笔记");
        // 已经是规范形状的，再解析一遍还是它
        assert_eq!(canonical("Example"), "Example");
    }

    #[test]
    fn underscores_are_spaces_and_whitespace_is_collapsed() {
        assert_eq!(canonical("hello_world"), "Hello world");
        assert_eq!(canonical("hello   world"), "Hello world");
        assert_eq!(canonical("  hello  world  "), "Hello world");
        // 全是下划线 → 规整完是空的 → 空标题
        assert!(reason("_").contains("标题不能为空"));
    }

    #[test]
    fn components_are_reordered_into_the_standard_order() {
        // 顺序天然自由，回显一律按「名称[@状态][#段落]」这个标准顺序来
        assert_eq!(canonical("a#小节@edit"), "A@edit#小节");
        assert_eq!(canonical("a@edit#小节"), "A@edit#小节");
        // 同名成分后者覆盖前者
        assert_eq!(canonical("bad#a#b"), "Bad#b");
        // 空成分算没写
        assert_eq!(canonical("x#"), "X");
        assert_eq!(canonical("x@"), "X");
    }

    #[test]
    fn viewing_the_latest_is_abbreviated_in_the_canonical_form() {
        // `@view` 指最新版，规范串里缩写掉；指了版本才写出来
        assert_eq!(canonical("示例笔记@view"), "示例笔记");
        assert_eq!(canonical("示例笔记@view-3"), "示例笔记@view-3");
        // 状态词不分大小写
        assert_eq!(canonical("示例笔记@EDIT"), "示例笔记@edit");
        assert_eq!(canonical("示例笔记@View-3"), "示例笔记@view-3");
    }

    #[test]
    fn references_are_tokens() {
        // 解析只收 token，不判断它是号数还是别的引用形式 —— 那是仓库那一层的事
        assert_eq!(canonical("a@view-3"), "A@view-3");
        assert_eq!(canonical("a@view-a1b2"), "A@view-a1b2");
        assert_eq!(canonical("a@rollback-abc"), "A@rollback-abc");
        assert_eq!(canonical("a@unlock"), "A@unlock");
        assert_eq!(canonical("a@unlock-3"), "A@unlock-3");
    }

    #[test]
    fn an_unknown_state_is_rejected_instead_of_being_dropped() {
        assert!(reason("示例笔记@edti").contains("不认识这个状态"));
        // 漏写版本：`@view-` 与 `@view`（最新版）不是一回事
        assert!(reason("示例笔记@view-").contains("要写版本"));
        assert!(reason("示例笔记@unlock-").contains("要写版本"));
        // 回退到最新版没有意义，必须指明版本
        assert!(reason("示例笔记@rollback").contains("要指明版本"));
    }

    #[test]
    fn special_is_a_virtual_namespace_without_state() {
        let it = parsed("special:settings");
        assert_eq!(it.address.namespace.id, "special");
        assert_eq!(it.address.namespace.spelling, "Special");
        assert_eq!(it.address.page, "Settings");
        assert_eq!(it.address.mode, Mode::View { reference: None });

        // 规范形状与大小写：前缀与页面名都首字母大写，查找大小写不敏感
        assert_eq!(canonical("special:all"), "Special:All");
        assert_eq!(canonical("SPECIAL:ALL"), "Special:All");
        // 章节保留
        assert_eq!(canonical("special:settings#外观"), "Special:Settings#外观");
        // 状态一律裁掉：特殊页面没有那些状态，回显的规范形状本身就是提示
        assert_eq!(canonical("special:newtab@edit"), "Special:Newtab");
        assert_eq!(canonical("special:all@view-3"), "Special:All");
        assert_eq!(canonical("special:all#小节@delete"), "Special:All#小节");
    }

    #[test]
    fn unknown_special_pages_and_namespaces_are_rejected() {
        let message = reason("special:nope");
        assert!(message.contains("没有这个特殊页面"), "{message}");
        assert!(
            message.contains("newtab"),
            "报错要列出现有的页面：{message}"
        );
        assert!(reason("special:").contains("要写页面名"));
        assert!(reason("foo:bar").contains("没有这个命名空间"));
    }

    #[test]
    fn illegal_titles_are_rejected() {
        assert!(reason("带<尖括号>").contains("不能有 <"));
        assert!(reason("x|y").contains("不能有 |"));
        let too_long = "长".repeat(200);
        assert!(reason(&too_long).contains("标题过长"));
    }

    #[test]
    fn canonical_addresses_reparse_to_themselves() {
        // 地址栏一回显就再解析一遍，所以这一条必须成立，否则回显自己把自己弄坏
        for input in [
            "示例笔记",
            "示例笔记@edit",
            "示例笔记@view-3",
            "示例笔记@unlock",
            "示例笔记@unlock-3",
            "示例笔记@rollback-3",
            "示例笔记@view-12#小节",
            "special:all",
            "Special:Settings#外观",
            "hello_world#小节",
        ] {
            let once = canonical(input);
            assert_eq!(canonical(&once), once, "{input:?} 的规范串应当幂等");
        }
    }

    /// 前后端之间的**线格式**。
    ///
    /// 前端 `src/bindings/address.ts` 是按这个形状手写的一份镜像 —— 序列化一变，
    /// 界面就会在运行时按错的分支走。所以在这里钉住它：改 `serde` 属性或字段名，
    /// 这条测试会先炸。
    #[test]
    fn the_wire_format_matches_the_frontend_mirror() {
        let wire = serde_json::to_value(parsed("示例笔记@view-3#小节")).unwrap();
        assert_eq!(wire["canonical"], "示例笔记@view-3#小节");
        assert_eq!(wire["address"]["namespace"]["id"], "0");
        assert_eq!(wire["address"]["namespace"]["spelling"], "");
        assert_eq!(wire["address"]["page"], "示例笔记");
        assert_eq!(wire["address"]["mode"]["kind"], "view");
        assert_eq!(wire["address"]["mode"]["ref"], "3");
        assert_eq!(wire["address"]["section"], "小节");

        let wire = serde_json::to_value(parsed("special:settings")).unwrap();
        assert_eq!(wire["address"]["namespace"]["spelling"], "Special");
        assert_eq!(wire["address"]["mode"]["kind"], "view");
        assert!(wire["address"]["mode"]["ref"].is_null());

        let wire = serde_json::to_value(parsed("a@unlock")).unwrap();
        assert_eq!(wire["address"]["mode"]["kind"], "unlock");
        assert!(wire["address"]["mode"]["ref"].is_null());

        let wire = serde_json::to_value(parsed("a@rollback-7")).unwrap();
        assert_eq!(wire["address"]["mode"]["kind"], "rollback");
        assert_eq!(wire["address"]["mode"]["ref"], "7");
    }
}
