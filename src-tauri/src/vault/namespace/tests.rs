//! 命名空间的测试。两类：表的规则（纯内存）与仓库上的效果（要开一个临时仓库）。

use super::*;
use crate::vault::{address, database::Database};

fn scratch(name: &str) -> Database {
    let dir =
        std::env::temp_dir().join(format!("refind-note-ns-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let workspace = crate::storage::workspace::Workspace::open(dir).unwrap();
    Database::open(&workspace).unwrap()
}

fn cleanup(database: &Database) {
    if let Some(root) = database.root().parent() {
        let _ = std::fs::remove_dir_all(root);
    }
}

#[test]
fn the_builtins_are_there_from_the_start() {
    let table = NamespaceTable::builtin();
    assert_eq!(table.name_of(MAIN_ID), "");
    assert_eq!(table.name_of(SPECIAL_ID), "special");
    assert_eq!(table.name_of(HELP_ID), "Help");
    assert!(
        table.is_virtual(HELP_ID),
        "帮助页的正文编在程序里，不在仓库"
    );
    assert!(table.get(TEMPLATE_ID).is_some());
    assert!(table.is_virtual(SPECIAL_ID));
    assert!(!table.is_virtual(TEMPLATE_ID));
}

#[test]
fn aliases_are_matched_loosely_but_displayed_canonically() {
    let mut table = NamespaceTable::builtin();
    table.add("manual", vec!["手册".to_string()], None).unwrap();

    let by_alias = table.lookup(" 手册 ").expect("别名应当认得出");
    let id = by_alias.id.clone();
    assert_eq!(
        table.lookup("MANUAL").map(|item| item.id.as_str()),
        Some(id.as_str())
    );

    // 回显用规范名：别名只是"也认"，不是它的名字
    let parsed = crate::vault::title::parse("手册:入门", &table).unwrap();
    assert_eq!(parsed.key(), format!("{id}:入门"));
    assert_eq!(parsed.display(&table), "manual:入门");
}

#[test]
fn names_and_aliases_cannot_collide() {
    let mut table = NamespaceTable::builtin();
    table.add("manual", Vec::new(), None).unwrap();
    assert!(table
        .add("Help", Vec::new(), None)
        .unwrap_err()
        .contains("已经存在"));
    assert!(table
        .add(
            "other",
            vec!["手册2".to_string(), "manual".to_string()],
            None
        )
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

    let created = database
        .add_namespace("manual", vec!["手册".to_string()], None)
        .unwrap();
    let id = created
        .iter()
        .find(|item| item.name == "manual")
        .unwrap()
        .id
        .clone();

    let title = database.create("manual:入门").unwrap();
    assert_eq!(title, "manual:入门");

    // 用别名访问：回显保留别名，落到仓库上仍然是同一篇
    assert!(!id.is_empty(), "刚建的命名空间该有个标识");
    let resolved = database
        .resolve_address("手册:入门")
        .unwrap()
        .expect("不是空输入");
    assert_eq!(resolved.canonical, "手册:入门");
    assert_eq!(
        resolved.outcome,
        crate::vault::resolve::Outcome::Note {
            title: "manual:入门".to_string()
        }
    );

    // 不带前缀的名字落在主命名空间，不会撞上 manual:入门
    let plain = database
        .resolve_address("入门")
        .unwrap()
        .expect("不是空输入");
    assert_eq!(
        plain.outcome,
        crate::vault::resolve::Outcome::Missing {
            title: "入门".to_string()
        }
    );

    cleanup(&database);
}

#[test]
fn rename_moves_no_files_but_rewrites_the_titles() {
    let database = scratch("rename");
    database.add_namespace("manual", Vec::new(), None).unwrap();
    database.create("manual:入门").unwrap();

    let id = database.id_of("manual:入门").expect("刚建的就该找得到");
    let before = database.log_path(&id);

    database.rename_namespace("manual", "手册").unwrap();

    // 文件一个不挪，键也不变 —— 变的只有显示标题
    assert_eq!(database.log_path(&id), before);
    assert!(before.is_file());
    assert!(database.exists("手册:入门"));
    assert_eq!(database.id_of("手册:入门").as_deref(), Some(id.as_str()));
    // 旧名字不认了
    assert!(!database.exists("manual:入门"));
    assert_eq!(database.display_of("手册:入门").unwrap(), "手册:入门");

    cleanup(&database);
}

#[test]
fn emptying_a_namespace_moves_its_notes_to_the_trash() {
    let database = scratch("empty");
    database.add_namespace("manual", Vec::new(), None).unwrap();
    database.create("manual:一").unwrap();
    database.create("manual:二").unwrap();
    database.create("留下来的").unwrap();

    let moved = database.empty_namespace("manual").unwrap();
    assert_eq!(moved, 2);

    // 命名空间还在，里面的页面进了回收站
    assert!(database.namespaces().lookup("manual").is_some());
    assert!(!database.exists("manual:一"));
    assert!(database.exists("留下来的"));

    // 删除本身会把名字挪进 trashed
    let titles = database.titles().unwrap();
    assert_eq!(
        titles
            .trashed
            .values()
            .filter(|name| name.starts_with("manual:"))
            .count(),
        2
    );

    cleanup(&database);
}

#[test]
fn deleting_a_namespace_empties_it_first() {
    let database = scratch("delete-ns");
    database.add_namespace("manual", Vec::new(), None).unwrap();
    database.create("manual:入门").unwrap();

    let moved = database.delete_namespace("manual").unwrap();
    assert_eq!(moved, 1);
    assert!(database.namespaces().lookup("manual").is_none());
    assert!(!database.exists("manual:入门"));

    cleanup(&database);
}

/// 老仓库的表里可能缺内建命名空间（`File` 是后加的）：打开时补上，且不动已有条目
#[test]
fn missing_builtins_are_filled_in_without_touching_the_rest() {
    // 手工造一张"老表"：只有主命名空间与一个用户自己建的
    let mut table = NamespaceTable {
        items: vec![
            NamespaceTable::builtin().items[0].clone(),
            Namespace {
                id: "ns1".to_string(),
                name: "笔记".to_string(),
                aliases: vec!["notes".to_string()],
                storable: true,
                site: None,
            },
        ],
        defaults_sown: true,
    };
    assert!(table.get("file").is_none(), "一开始确实缺");

    assert!(table.ensure_builtins(), "应当补上了东西");
    assert!(table.get(FILE_ID).is_some(), "补上了 File");
    assert!(table.get(HELP_ID).is_some());
    assert!(table.get(TEMPLATE_ID).is_some());

    // 用户自己那条一个字节没动
    let mine = table.get("ns1").unwrap();
    assert_eq!(mine.name, "笔记");
    assert_eq!(mine.aliases, vec!["notes".to_string()]);

    // 再补一次：没有变化
    assert!(!table.ensure_builtins());
}

#[test]
fn a_cross_site_namespace_holds_no_pages() {
    let database = scratch("cross-site");
    // 默认播下来的那两个跨站命名空间就该在（新仓库）
    let seeded = database.namespaces();
    assert!(seeded.lookup("zhwiki").is_some(), "新仓库该种下 zhwiki");
    assert!(seeded.lookup("求闻百科").is_some(), "别名也认");

    database
        .add_namespace(
            "elsewhere",
            vec!["别处".to_string()],
            Some("https://example.org/wiki/$1".to_string()),
        )
        .unwrap();

    let table = database.namespaces();
    let found = table.lookup("别处").unwrap();
    assert!(found.is_cross_site());
    assert!(!found.storable);
    // 本仓库里建不了它：页面在别人家
    assert!(database.create("elsewhere:某页").is_err());
    assert!(crate::vault::title::parse("elsewhere:某页", &table).is_err());

    // 但**写成地址**解析得出来：它是个地址，只是落到"交给浏览器打开"
    let parsed = address::parse("elsewhere:某页", &table).unwrap().unwrap();
    assert_eq!(parsed.canonical, "elsewhere:某页");
    let resolved = database
        .resolve_address("elsewhere:某页")
        .unwrap()
        .expect("不是空输入");
    assert_eq!(
        resolved.outcome,
        crate::vault::resolve::Outcome::CrossSite {
            title: "elsewhere:某页".to_string(),
            url: "https://example.org/wiki/某页".to_string(),
        }
    );

    cleanup(&database);
}