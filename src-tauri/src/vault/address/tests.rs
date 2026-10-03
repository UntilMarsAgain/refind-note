//! 地址语法的测试。
//!
//! 这一层的规矩是"拼写必须唯一"：同一个意思只有一个写法，规范串与解析结果要能对上。

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
