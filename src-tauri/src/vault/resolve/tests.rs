//! 落点的用例：地址栏那行字该落到哪一页，以及"某一版"的读 / 导出 / 解锁行为。
//!
//! 断言都落在**对外可见的结论**上（`Outcome`、`canonical`、`editable`、`via`）或
//! `read_note` 的返回上，不去碰内部分支 —— 这样重排这一层的内部结构时用例不用跟着动。

use super::{Outcome, Via};
use crate::vault::address::Mode;
use crate::vault::database::Database;
use crate::vault::notes::Reading;

fn scratch(name: &str) -> Database {
    let dir = std::env::temp_dir().join(format!(
        "refind-note-resolve-test-{}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let workspace = crate::storage::workspace::Workspace::open(dir).unwrap();
    Database::open(&workspace).unwrap()
}

fn cleanup(database: &Database) {
    // 库在 `<暂存目录>/db` 下，settings 是它的兄弟目录 —— 从暂存根整棵删掉
    if let Some(root) = database.root().parent() {
        let _ = std::fs::remove_dir_all(root);
    }
}

/// 解锁要**当场验**：错的口令不该被当成解开了 ——
/// 不验的话界面以为解开了、去读却读不出来，"再输一次"这条路就断了
#[test]
fn unlocking_checks_the_passphrase_right_away() {
    // 碰会话口令的用例要串行（锁是全局那一把）
    let _guard = crate::storage::session::test_lock::guard();
    let database = scratch("unlock-check");
    let policy = crate::storage::codec::Policy {
        compress: true,
        symmetric: true,
        ..Default::default()
    };
    database.create("密文").unwrap();
    database
        .commit_with(
            "密文",
            "正文",
            None,
            Some(policy),
            Some("对的口令".to_string()),
        )
        .unwrap();
    crate::storage::session::forget_all();

    // 错的：报"口令不对"，而且不留下任何东西（再输一次才可能）
    let error = database.unlock("密文", None, "错的口令").unwrap_err();
    assert!(error.contains("口令不对"), "{error}");
    assert!(!database.passphrase_stored("密文", None).unwrap());
    assert!(matches!(
        database.read_note("密文", None).unwrap(),
        Reading::Locked { .. }
    ));

    // 对的：解开，读得动
    database.unlock("密文", None, "对的口令").unwrap();
    assert!(database.passphrase_stored("密文", None).unwrap());
    assert!(matches!(
        database.read_note("密文", None).unwrap(),
        Reading::Ready { .. }
    ));

    crate::storage::session::forget_all();
    cleanup(&database);
}

/// 导出：写到用户给的位置，内容就是 markdown **原文**（连模板记号一起带走）
#[test]
fn exporting_writes_the_markdown_source() {
    let database = scratch("export");
    database.create("导出我").unwrap();
    database
        .commit("导出我", "# 标题\n\n::banner text=\"一条\"\n", None)
        .unwrap();

    let target =
        std::env::temp_dir().join(format!("refind-note-export-test-{}.md", std::process::id()));
    database
        .export_note("导出我", None, target.as_path())
        .unwrap();

    let written = std::fs::read_to_string(&target).unwrap();
    assert_eq!(written, "# 标题\n\n::banner text=\"一条\"\n");

    // 取不到的位置：报错，而不是悄悄当成功
    assert!(database
        .export_note("导出我", None, std::path::Path::new("/没有这个目录/也/不许/写.md"))
        .is_err());

    let _ = std::fs::remove_file(&target);
    cleanup(&database);
}

/// 写一篇指令页
fn command_page(database: &Database, title: &str, body: &str) {
    database.create(title).unwrap();
    database
        .commit(title, &format!("$$COMMAND$$\n{body}\n"), None)
        .unwrap();
}

/// 打开指令页 → 落到它指向的那一页，并且记得"从哪儿来"
#[test]
fn a_command_page_chases_to_its_target() {
    let database = scratch("chase");
    database.create("目标").unwrap();
    database.commit("目标", "正文", None).unwrap();
    command_page(&database, "跳板", "REDIRECT: 目标");

    let resolved = database.resolve_address("跳板").unwrap().unwrap();
    assert_eq!(
        resolved.outcome,
        Outcome::Note {
            title: "目标".to_string()
        }
    );
    assert_eq!(
        resolved.via,
        Some(Via {
            from: "跳板".to_string(),
            random: false
        })
    );

    // `@no-command`：不跟跳，看这一页自己
    let kept = database
        .resolve_address("跳板@no-command")
        .unwrap()
        .unwrap();
    assert_eq!(
        kept.outcome,
        Outcome::Note {
            title: "跳板".to_string()
        }
    );
    assert_eq!(kept.via, None);

    // 编辑、看历史这些状态也不跟跳 —— 否则会改错页面
    for state in ["跳板@edit", "跳板@history", "跳板@delete"] {
        let resolved = database.resolve_address(state).unwrap().unwrap();
        assert_eq!(
            resolved.outcome,
            Outcome::Note {
                title: "跳板".to_string()
            },
            "{state} 不该跟着跳"
        );
    }

    cleanup(&database);
}

/// 读指令页：正文按**代码块**看，并带上指令信息
#[test]
fn a_command_page_reads_as_a_code_block_with_its_info() {
    let database = scratch("command-read");
    command_page(&database, "跳板", "REDIRECT: 别处");

    let Reading::Ready { note } = database
        .read_note("跳板@no-command".trim_end_matches("@no-command"), None)
        .unwrap()
    else {
        panic!("应当读得到");
    };
    let info = note.command.as_ref().expect("应当有指令信息");
    assert_eq!(info.kind, "redirect");
    assert_eq!(info.label, "重定向");
    assert_eq!(info.argument, "别处");
    assert!(
        note.html.contains("<pre") || note.html.contains("<code"),
        "指令页的正文应当按代码块渲染：{}",
        note.html
    );

    cleanup(&database);
}

/// 跳转绕成环：报一句能看懂的错，而不是递归到栈溢出
#[test]
fn a_redirect_loop_is_reported() {
    let database = scratch("loop");
    command_page(&database, "甲", "REDIRECT: 乙");
    command_page(&database, "乙", "REDIRECT: 甲");

    let error = database.resolve_address("甲").unwrap_err();
    assert!(error.contains("环"), "{error}");

    cleanup(&database);
}

/// 指令写坏了要报出来 —— 不能静静显示成正文
#[test]
fn a_broken_command_is_reported() {
    let database = scratch("broken");
    command_page(&database, "坏的", "随便写点什么");

    let error = database.resolve_address("坏的").unwrap_err();
    assert!(error.contains("认不出来"), "{error}");

    // 只有标记、第二行都没写
    database.create("空的").unwrap();
    database.commit("空的", "$$COMMAND$$", None).unwrap();
    let error = database.resolve_address("空的").unwrap_err();
    assert!(error.contains("没写指令"), "{error}");

    cleanup(&database);
}

/// 随机重定向：落在指定命名空间里的某一篇，并且标明这是随机来的
#[test]
fn random_redirect_lands_somewhere_in_that_namespace() {
    let database = scratch("random-command");
    database.add_namespace("manual", Vec::new(), None).unwrap();
    database.create("manual:甲").unwrap();
    database.create("manual:乙").unwrap();
    command_page(&database, "随机", "RANDOM_REDIRECT: manual");

    let resolved = database.resolve_address("随机").unwrap().unwrap();
    let Outcome::Note { title } = &resolved.outcome else {
        panic!("应当落到一篇笔记上");
    };
    assert!(title == "manual:甲" || title == "manual:乙", "{title}");
    let via = resolved.via.expect("随机跳转也要记下从哪儿来");
    assert!(via.random, "界面据此说「来自随机跳转」");

    cleanup(&database);
}

#[test]
fn an_existing_note_resolves_to_it() {
    let database = scratch("existing");
    database.create("示例").unwrap();

    let resolved = database.resolve_address("示例@edit#小节").unwrap().unwrap();
    assert!(matches!(resolved.outcome, Outcome::Note { ref title } if title == "示例"));
    // 这一页存在，状态与章节都保留
    assert_eq!(resolved.canonical, "示例@edit#小节");
    assert!(matches!(resolved.address.mode, Mode::Edit));

    cleanup(&database);
}

#[test]
fn a_missing_note_drops_its_state() {
    let database = scratch("missing");

    let resolved = database.resolve_address("没有的@edit").unwrap().unwrap();
    assert!(matches!(resolved.outcome, Outcome::Missing { ref title } if title == "没有的"));
    assert_eq!(resolved.canonical, "没有的", "还不存在的页不保留状态");

    cleanup(&database);
}

#[test]
fn special_pages_resolve_to_their_view() {
    let database = scratch("special");

    let resolved = database
        .resolve_address("special:settings#外观")
        .unwrap()
        .unwrap();
    assert!(matches!(resolved.outcome, Outcome::Special { ref page } if page == "settings"));
    assert_eq!(resolved.canonical, "Special:Settings#外观");

    cleanup(&database);
}

#[test]
fn random_lands_on_a_note_and_needs_one_to_exist() {
    let database = scratch("random");
    assert!(database
        .resolve_address("special:random")
        .unwrap_err()
        .contains("还没有笔记"));

    database.create("甲").unwrap();
    database.create("乙").unwrap();
    let resolved = database.resolve_address("special:random").unwrap().unwrap();
    assert!(
        matches!(resolved.outcome, Outcome::Note { .. }),
        "{resolved:?}"
    );
    assert!(
        resolved.canonical == "甲" || resolved.canonical == "乙",
        "落点该是现有两篇之一：{}",
        resolved.canonical
    );

    cleanup(&database);
}

#[test]
fn a_version_token_is_read_as_a_number() {
    let database = scratch("token");
    database.create("甲").unwrap();
    database.commit("甲", "第一版", None).unwrap();
    database.commit("甲", "第二版", None).unwrap();

    // 数字 token：读到那一版
    assert!(matches!(
        database.read_note("甲", Some("1")).unwrap(),
        Reading::Ready { note } if note.markdown == "第一版"
    ));
    // 不像数字的 token：说清楚是哪里不对
    assert!(database
        .read_note("甲", Some("abc"))
        .unwrap_err()
        .contains("要写数字"));

    cleanup(&database);
}