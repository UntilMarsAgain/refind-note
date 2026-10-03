//! 笔记的测试。
//!
//! 被测的方法都挂在 `Database` 上，而**固有方法不随模块走** —— 所以这里不需要
//! `use super::*`：调用处写 `database.read(...)`，它在哪个文件都照样解析得到。
//! 用得到的自由函数与类型按需导入。

// 口令在测试里是现解锁的，而解锁那一手住在会话存储那边 —— 各实现文件按需导入它，
// 测试这里也自己带一份。
use crate::storage::session;

use super::*;
use crate::storage::codec::Policy;
use crate::vault::database::{Config, Database};

fn scratch(name: &str) -> Database {
    let dir = std::env::temp_dir().join(format!(
        "refind-note-notes-test-{}-{name}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    let workspace = crate::storage::workspace::Workspace::open(dir).unwrap();
    Database::open(&workspace).unwrap()
}

fn cleanup(database: &Database) {
    // 库在 `<暂存目录>/db` 下，settings 是它的兄弟目录 —— 从暂存根整棵删掉
    if let Some(root) = database.root().parent() {
        let _ = fs::remove_dir_all(root);
    }
}

/// **加密的模板页：说清是"没解锁"，不是"没这一页"** —— 而且**不许弹口令**。
///
/// 这一条是从真事上来的：默认策略带 GPG 加密的人，每一页都是加密的；
/// 嵌入时若直接去解密，gpg 会叫出 pinentry 把界面挂在那儿等人输密码
/// （看起来就是"程序死了"）。所以渲染时一律走非交互（见 `codec::without_prompting`），
/// 拿不到就把原因写在页面上。
#[test]
fn a_locked_template_says_it_is_locked_instead_of_hanging() {
    let _guard = session_guard();

    let database = scratch("template-locked");
    let locked = Policy {
        compress: true,
        symmetric: true,
        ..Policy::default()
    };
    database.create("Template:锁住的").unwrap();
    database
        .commit_with(
            "Template:锁住的",
            "**锁住的卡片**",
            None,
            Some(locked.clone()),
            Some("口令".to_string()),
        )
        .unwrap();

    // 这一趟已经解锁过了（提交时顺延）：嵌得进来
    let ok = database
        .render_html("::html src=\"锁住的\"\n", "某页")
        .unwrap();
    assert!(ok.contains("锁住的卡片"), "{ok}");

    // 把口令从这一趟里忘掉（等于"换台机器/重启之后"）：不再解得开
    let id = database.locate("Template:锁住的").unwrap();
    session::forget(&id, 1);

    let locked_out = database
        .render_html("::html src=\"锁住的\"\n", "某页")
        .unwrap();
    assert!(locked_out.contains("template--problem"), "{locked_out}");
    assert!(
        locked_out.contains("加密"),
        "要说清是没解锁，而不是含糊的取不到：{locked_out}"
    );
    assert!(
        !locked_out.contains("不存在"),
        "它明明在，别说成不存在：{locked_out}"
    );

    cleanup(&database);
}

/// **用户模板按名字用**：`::卡片` 找的就是 `Template:卡片`（不是"未知模板"）。
///
/// 这正是用户报的那条："模板嵌入无法搜索到模板命名空间的用户定义的模板" ——
/// 名字只在内置表里找，于是自定义的模板一律被报成"未知模板"。
#[test]
fn a_user_template_is_invoked_by_its_name() {
    let database = scratch("template-by-name");

    database.create("Template:名片").unwrap();
    database
        .commit(
            "Template:名片",
            "**{{谁}}** 的名片\n\n{{body}}",
            None,
        )
        .unwrap();

    database.create("某页").unwrap();
    let source = "::名片 谁=\"甲\"\n  这一行是正文\n";
    let html = database.render_html(source, "某页").unwrap();
    assert!(!html.contains("template--unknown"), "不该报未知模板：{html}");
    assert!(html.contains("<strong>甲</strong>"), "参数要填进去：{html}");
    assert!(html.contains("这一行是正文"), "块内容要填进 {{{{body}}}}：{html}");

    // 内置的优先：名字撞上时按内置的算
    database.create("Template:quote").unwrap();
    database.commit("Template:quote", "用户写的 quote", None).unwrap();
    let quote = database.render_html("::quote\n  一句引文\n", "某页").unwrap();
    assert!(quote.contains("quote__origin") || quote.contains("class=\"quote\""), "{quote}");
    assert!(!quote.contains("用户写的 quote"), "内置的该赢：{quote}");

    // 真没有这一页：还是报未知模板（那才是"未知"）
    let unknown = database.render_html("::没有这张\n", "某页").unwrap();
    assert!(unknown.contains("template--unknown"), "{unknown}");

    cleanup(&database);
}

/// 模板自己嵌自己：**不能转圈**（展开到上限就停下来说一句）
#[test]
fn a_template_that_embeds_itself_stops() {
    let database = scratch("template-loop");

    database.create("Template:圈").unwrap();
    database.commit("Template:圈", "::圈\n", None).unwrap();
    database.create("某页").unwrap();

    let html = database.render_html("::圈\n", "某页").unwrap();
    assert!(html.contains("嵌套"), "要说清是套得太深：{html}");

    cleanup(&database);
}

/// 模板页提交了新版本：缓存要跟着失效（不然嵌进来的是上一版）
#[test]
fn an_embedded_template_follows_the_newest_version() {
    let database = scratch("template-fresh");

    database.create("Template:卡片").unwrap();
    database.commit("Template:卡片", "第一版", None).unwrap();
    let first = database
        .render_html("::html src=\"卡片\"\n", "某页")
        .unwrap();
    assert!(first.contains("第一版"), "{first}");

    database.commit("Template:卡片", "第二版", None).unwrap();
    let second = database
        .render_html("::html src=\"卡片\"\n", "某页")
        .unwrap();
    assert!(second.contains("第二版"), "缓存要跟着版本走：{second}");
    assert!(!second.contains("第一版"), "{second}");

    cleanup(&database);
}

/// **模板嵌入**：`src=` 指的是 `Template:` 命名空间里的一页，真的取得到。
///
/// 这一条是从一个"看着像坏了"的地方补上的：`src=` 那条路以前**永远**返回
/// "取不到它"（谁也没实现），于是用户定义的模板一个都嵌不进来。
#[test]
fn a_template_page_can_be_embedded_by_name() {
    let database = scratch("template-src");

    // 用户自定义的模板：`Template:` 里的一页
    database.create("Template:嵌入测试卡片").unwrap();
    database
        .commit("Template:嵌入测试卡片", "<b>嵌进来的卡片</b>", None)
        .unwrap();

    // 三种写法都该认
    for name in [
        "嵌入测试卡片",
        "Template:嵌入测试卡片",
        "template:嵌入测试卡片",
        "  嵌入测试卡片  ",
    ] {
        let html = database
            .render_html(&format!("::html src=\"{name}\"\n"), "某页")
            .unwrap();
        assert!(html.contains("嵌进来的卡片"), "src={name:?} 没取到：{html}");
        assert!(!html.contains("template--problem"), "src={name:?}：{html}");
    }

    // 没有这一页：摆问题框，把话说清楚
    let missing = database
        .render_html("::html src=\"没有这张卡\"\n", "某页")
        .unwrap();
    assert!(missing.contains("template--problem"), "{missing}");

    // 普通命名空间里的页面**不是**模板页：`src=` 取不到它（只认 `Template:`）
    database.create("一张普通笔记").unwrap();
    database.commit("一张普通笔记", "不该被嵌进来", None).unwrap();
    let ordinary = database
        .render_html("::html src=\"一张普通笔记\"\n", "某页")
        .unwrap();
    assert!(!ordinary.contains("不该被嵌进来"), "{ordinary}");

    cleanup(&database);
}

/// 口令是**进程内共享**的（`session`）：用那把全局的锁，别的模块也一样拿它
fn session_guard() -> std::sync::MutexGuard<'static, ()> {
    crate::storage::session::test_lock::guard()
}

/// 数一数字节仓里有几个 blob。
///
/// 只为"草稿不进仓"那条用例 —— 生产代码里不必为了测试留一个遍历接口，
/// 那个等 GC 的时候再写。
fn blob_count(database: &Database) -> usize {
    let root = database.root().join("blobs");
    let mut count = 0;
    let Ok(shards) = fs::read_dir(&root) else {
        return 0;
    };
    for shard in shards.flatten() {
        let Ok(files) = fs::read_dir(shard.path()) else {
            continue;
        };
        for file in files.flatten() {
            if !file.file_name().to_string_lossy().ends_with(".tmp") {
                count += 1;
            }
        }
    }
    count
}

#[test]
fn a_note_is_created_committed_and_read_back() {
    let _guard = session_guard();
    let database = scratch("roundtrip");
    session::forget_all();

    database.create("示例笔记").unwrap();
    assert!(database.exists("示例笔记"));

    let note = database
        .commit("示例笔记", "# 标题\n\n正文", Some("第一次".to_string()))
        .unwrap();
    assert_eq!(note.title, "示例笔记");
    assert_eq!(note.rev, 1);
    assert_eq!(note.markdown, "# 标题\n\n正文");
    assert_eq!(note.summary.as_deref(), Some("第一次"));
    assert!(!note.created.is_empty() && !note.modified.is_empty());

    // 再读一遍走的是日志 → blob 这条路
    let again = database.read("示例笔记").unwrap();
    assert_eq!(again.markdown, note.markdown);
    assert_eq!(again.created, note.created);

    // 提交两次就是两版
    database
        .commit("示例笔记", "# 标题\n\n改过了", None)
        .unwrap();
    let latest = database.read("示例笔记").unwrap();
    assert_eq!(latest.rev, 2);
    assert_eq!(latest.markdown, "# 标题\n\n改过了");

    cleanup(&database);
}

#[test]
fn titles_are_unique_and_errors_are_explained() {
    let database = scratch("unique");

    database.create("甲").unwrap();
    let error = database.create("甲").unwrap_err();
    assert!(error.contains("已经有一篇"), "{error}");

    assert!(database.create("   ").is_err());
    assert!(database.read("乙").unwrap_err().contains("没有这篇"));

    cleanup(&database);
}

#[test]
fn a_created_title_is_stored_in_the_shape_the_parser_looks_up() {
    let _guard = session_guard();
    let database = scratch("canonical");
    session::forget_all();

    // 建的时候写小写、下划线，存下来必须是规范形状 ——
    // 否则"建了 example_note、地址栏敲 Example note"会找不到自己
    let created = database.create("example_note").unwrap();
    assert_eq!(created, "Example note");
    assert!(database.exists("Example note"));
    // 换个写法也找得到：地址解析与这里是同一把尺子
    assert!(database.exists("example_note"));

    // 名字本身不合法的建不出来 —— 否则会造出一篇打不开的笔记
    assert!(database.create("带<尖括号>").is_err());
    // 冒号前缀现在都不是可存储的命名空间
    assert!(database.create("foo:bar").is_err());
    assert!(database.create("special:all").is_err());

    cleanup(&database);
}

#[test]
fn the_layout_lands_on_the_first_open() {
    let _guard = session_guard();
    let database = scratch("layout");
    session::forget_all();

    // 该在的东西第一次打开就该在：主命名空间的日志目录与回收站目录
    assert!(database.root().join("objects").join("0").is_dir());
    assert!(database.root().join("trash").join("0").is_dir());

    cleanup(&database);
}

#[test]
fn a_created_note_can_always_be_reached_through_the_address_bar() {
    let _guard = session_guard();
    let database = scratch("openable");
    session::forget_all();

    // 用非规范写法建：小写、下划线
    let created = database.create("my_new_note").unwrap();
    assert_eq!(created, "My new note");

    // 关键一条：建完必须能被**地址解析**找回来 —— 创建与解析过的是同一把尺子，
    // 所以人怎么写都行，不会出现"建得出来、却永远打不开"的笔记。
    for input in [
        "my_new_note",
        "My new note",
        "my new note",
        "  My   new  note  ",
    ] {
        let parsed = crate::vault::address::parse(input, &database.namespaces())
            .unwrap_or_else(|reason| panic!("{input:?} 本该解析成功：{reason}"))
            .unwrap_or_else(|| panic!("{input:?} 不是空输入，应当是一个地址"));

        assert_eq!(
            parsed.address.page, created,
            "{input:?} 解析出来的页面名应当就是刚建的那一篇"
        );
        assert!(database.exists(&parsed.address.page));
    }

    cleanup(&database);
}

#[test]
fn the_list_carries_times_derived_from_the_log() {
    let _guard = session_guard();
    let database = scratch("list");
    session::forget_all();

    database.create("甲").unwrap();
    database.create("乙").unwrap();
    database.commit("甲", "内容", None).unwrap();

    let listed = database.list().unwrap();
    assert_eq!(listed.len(), 2);

    let first = listed.iter().find(|item| item.title == "甲").unwrap();
    assert_eq!(first.rev, 1);
    assert!(!first.created.is_empty() && !first.modified.is_empty());
    assert_eq!(first.key, "0:甲");

    cleanup(&database);
}

#[test]
fn a_draft_lives_in_its_own_slot_and_never_enters_the_blob_store() {
    let _guard = session_guard();
    let database = scratch("draft");
    session::forget_all();

    database.create("甲").unwrap();
    database.commit("甲", "提交过的内容", None).unwrap();
    let blobs_before = blob_count(&database);

    database.save_draft("甲", "还没定稿").unwrap();
    database.save_draft("甲", "又改了一版").unwrap();

    // 草稿可覆盖，且一个 blob 都不多
    assert_eq!(blob_count(&database), blobs_before);

    let draft = database.load_draft("甲").unwrap().unwrap();
    assert_eq!(draft.markdown, "又改了一版");
    // 封装照这篇当前的保护（新笔记 = 仓库默认），所以是压缩
    assert!(draft.protection.compress);

    // 草稿不影响正文
    assert_eq!(database.read("甲").unwrap().markdown, "提交过的内容");

    assert!(database.discard_draft("甲").unwrap());
    assert!(database.load_draft("甲").unwrap().is_none());
    assert!(!database.discard_draft("甲").unwrap());

    cleanup(&database);
}

#[test]
fn a_draft_goes_through_the_same_layers_as_a_commit() {
    let _guard = session_guard();
    let database = scratch("draft-sealed");
    session::forget_all();

    // 仓库默认是"压缩 + 口令对称"（新笔记从它出发）
    database
        .save_config(&Config {
            protection: Policy {
                compress: true,
                symmetric: true,
                ..Default::default()
            },
            ..Default::default()
        })
        .unwrap();
    database.create("甲").unwrap();

    // 没给口令：写不进去，说得清是缺口令
    let error = database.save_draft("甲", "秘密").unwrap_err();
    assert!(error.contains("口令"), "{error}");

    // 口令按页存，所以要给**这一篇**解锁
    let id = database.id_of("甲").unwrap();
    session::unlock(&id, 1, "pw".to_string());
    database.save_draft("甲", "秘密").unwrap();

    // 槽位里翻不到原文，但头是明文 —— 不输口令也知道它是加密的
    let raw = fs::read(database.draft_path(&id)).unwrap();
    assert!(!raw.windows(6).any(|window| window == "秘密".as_bytes()));

    let draft = database.load_draft("甲").unwrap().unwrap();
    assert!(draft.protection.symmetric && draft.protection.compress);
    assert_eq!(draft.markdown, "秘密");

    // 上了锁就读不出来
    session::forget_all();
    assert!(database.load_draft("甲").is_err());

    cleanup(&database);
}

#[test]
fn a_commit_inherits_the_notes_current_protection() {
    let _guard = session_guard();
    let database = scratch("protection");
    session::forget_all();

    database.create("甲").unwrap();

    // 显式给一次 = 换保护：这一版起用原样（仓库默认本来是压缩）
    let plain = database
        .commit_with("甲", "明文", None, Some(Policy::default()), None)
        .unwrap();
    assert!(plain.protection.is_plain());

    // 之后不给策略的提交**照这篇当前的保护**，不会悄悄换回仓库默认
    let again = database.commit("甲", "还是明文", None).unwrap();
    assert!(again.protection.is_plain(), "不给策略不会悄悄换封装");

    // 再显式换一次：从这一版起照新的粘住
    let packed = database
        .commit_with(
            "甲",
            "压过的内容，长一点好看出效果",
            None,
            Some(Policy {
                compress: true,
                ..Default::default()
            }),
            None,
        )
        .unwrap();
    assert!(packed.protection.compress);
    assert!(database.read("甲").unwrap().protection.compress);

    cleanup(&database);
}

#[test]
fn a_locked_page_says_so_instead_of_failing() {
    let _guard = session_guard();
    let database = scratch("locked");
    session::forget_all();

    database
        .save_config(&Config {
            protection: Policy {
                symmetric: true,
                ..Default::default()
            },
            ..Default::default()
        })
        .unwrap();
    database.create("甲").unwrap();

    let id = database.id_of("甲").unwrap();
    // 口令按**版本**存，所以提交时把它记给新写下的那一版
    database
        .commit_with("甲", "秘密内容", None, None, Some("pw".to_string()))
        .unwrap();

    // 上锁之后再读：**不是报错**，而是明说"这一页需要口令"
    session::forget_all();
    assert!(matches!(
        database.read_for_display("甲").unwrap(),
        Reading::Locked { protection, .. } if protection.symmetric
    ));

    // 给这一版解锁就正常了
    session::unlock(&id, 1, "pw".to_string());
    assert!(matches!(
        database.read_for_display("甲").unwrap(),
        Reading::Ready { note } if note.markdown == "秘密内容"
    ));

    // 口令按页存：给别的页解锁，不等于这一页能读
    session::forget_all();
    session::unlock("别的页的标识", 1, "pw".to_string());
    assert!(matches!(
        database.read_for_display("甲").unwrap(),
        Reading::Locked { .. }
    ));

    cleanup(&database);
}

#[test]
fn a_wrong_passphrase_can_be_tried_again() {
    let _guard = session_guard();
    let database = scratch("wrong-passphrase");
    session::forget_all();

    database
        .save_config(&Config {
            protection: Policy {
                symmetric: true,
                ..Default::default()
            },
            ..Default::default()
        })
        .unwrap();
    database.create("甲").unwrap();

    let id = database.id_of("甲").unwrap();
    database
        .commit_with("甲", "秘密内容", None, None, Some("right".to_string()))
        .unwrap();
    session::forget(&id, 1);

    // 输错：要说得出"这一次是错的"，否则界面没法让人重输
    session::unlock(&id, 1, "wrong".to_string());
    let reading = database.read_for_display("甲").unwrap();
    assert!(
        matches!(
            reading,
            Reading::Locked {
                wrong_passphrase: true,
                ..
            }
        ),
        "{reading:?}"
    );

    // 错的已经丢掉 —— 所以"再输一次"这件事才成立
    assert!(session::passphrase_for(&id, 1).is_none());

    // 输对就正常
    session::unlock(&id, 1, "right".to_string());
    assert!(matches!(
        database.read_for_display("甲").unwrap(),
        Reading::Ready { note } if note.markdown == "秘密内容"
    ));

    cleanup(&database);
}

#[test]
fn a_draft_is_never_signed_even_when_the_policy_says_so() {
    let _guard = session_guard();
    let database = scratch("draft-unsigned");
    session::forget_all();

    // 策略里的签名只对提交有意义。这里给一把**不存在**的密钥：草稿要是真去签，
    // 保存就会失败在"找不到签名密钥"上 —— 存得下来才是对的。
    database
        .save_config(&Config {
            protection: Policy {
                compress: true,
                gpg_sign: Some("不存在@example".to_string()),
                ..Default::default()
            },
            ..Default::default()
        })
        .unwrap();
    database.create("甲").unwrap();

    database.save_draft("甲", "草稿内容").unwrap();
    let draft = database.load_draft("甲").unwrap().unwrap();
    assert_eq!(draft.markdown, "草稿内容");
    assert!(draft.protection.compress, "压缩照旧");
    assert!(draft.protection.sign.is_none(), "草稿不该签名");

    cleanup(&database);
}

#[test]
fn a_locked_revision_reports_its_own_protection() {
    let _guard = session_guard();
    let database = scratch("revision-locked");
    session::forget_all();

    // 第 1 版加密、第 2 版明文：两版的保护头不一样
    database
        .save_config(&Config {
            protection: Policy {
                symmetric: true,
                ..Default::default()
            },
            ..Default::default()
        })
        .unwrap();
    database.create("甲").unwrap();
    let id = database.id_of("甲").unwrap();
    database
        .commit_with("甲", "秘密", None, None, Some("pw".to_string()))
        .unwrap();
    // 显式换回明文（不给策略的话会照第 1 版继续加密）
    database
        .commit_with("甲", "明文", None, Some(Policy::default()), None)
        .unwrap();

    // 给第 1 版一把**错**的口令：报出来的保护头必须是第 1 版自己的（加密），
    // 而不是最新一版那份（明文）
    session::forget_all();
    session::unlock(&id, 1, "wrong".to_string());
    match database.read_revision_for_display("甲", 1).unwrap() {
        Reading::Locked {
            protection,
            wrong_passphrase,
        } => {
            assert!(protection.symmetric, "保护头该来自第 1 版（加密的那版）");
            assert!(wrong_passphrase);
        }
        other => panic!("口令错了应当报上锁，而不是 {other:?}"),
    }

    cleanup(&database);
}

#[test]
fn a_reencoded_rollback_keeps_the_notes_current_protection() {
    let _guard = session_guard();
    let database = scratch("rollback-reencode");
    session::forget_all();

    database.create("甲").unwrap();
    database
        .commit_with("甲", "第一版", None, Some(Policy::default()), None)
        .unwrap(); // 第 1 版：明文
    database
        .commit_with(
            "甲",
            "第二版",
            None,
            Some(Policy {
                compress: true,
                ..Default::default()
            }),
            None,
        )
        .unwrap(); // 第 2 版：显式换成压缩 —— 这篇当前是压缩

    let after = database
        .rollback("甲", 1, Some("回到第一版".to_string()))
        .unwrap();

    // 内容回到第一版，但封装照**当前**的保护（压缩），不是第一版那份明文
    assert_eq!(after.rev, 3);
    assert_eq!(after.markdown, "第一版");
    assert!(after.protection.compress, "重编码的回滚保持这篇当前的保护");

    cleanup(&database);
}

#[test]
fn a_copy_rollback_reuses_the_blob_without_unlocking() {
    let _guard = session_guard();
    let database = scratch("rollback-copy");
    session::forget_all();

    database
        .save_config(&Config {
            protection: Policy {
                symmetric: true,
                ..Default::default()
            },
            ..Default::default()
        })
        .unwrap();
    database.create("甲").unwrap();
    let id = database.id_of("甲").unwrap();
    database
        .commit_with("甲", "第一版", None, None, Some("pw".to_string()))
        .unwrap();
    database.commit("甲", "第二版", None).unwrap(); // 照当前保护：继续加密

    // 不解锁也能复制回滚：不是读内容，只是把第 1 版那份封装再指一次
    session::forget_all();
    let new_rev = database
        .rollback_copy("甲", 1, Some("回到第一版".to_string()))
        .unwrap();
    assert_eq!(new_rev, 3);

    // 新头是加密的（照第 1 版那份），解开之后正是第一版的内容
    assert!(matches!(
        database.read_for_display("甲").unwrap(),
        Reading::Locked { protection, .. } if protection.symmetric
    ));
    session::unlock(&id, 3, "pw".to_string());
    assert_eq!(database.read("甲").unwrap().markdown, "第一版");

    cleanup(&database);
}

#[test]
fn rolling_back_writes_a_new_commit_and_keeps_the_old_ones() {
    let _guard = session_guard();
    let database = scratch("rollback");
    session::forget_all();

    database.create("甲").unwrap();
    database.commit("甲", "第一版", None).unwrap();
    database.commit("甲", "第二版", None).unwrap();

    let after = database
        .rollback("甲", 1, Some("回到第一版".to_string()))
        .unwrap();

    // 回退是**新提交**，不是把历史砍回去
    assert_eq!(after.rev, 3);
    assert_eq!(after.markdown, "第一版");
    assert_eq!(after.summary.as_deref(), Some("回到第一版"));

    // 旧记录一条都没动，所以回退本身也能再被回退
    let history = database.revisions_of("甲").unwrap();
    assert_eq!(history.len(), 3);
    assert_eq!(database.read_revision("甲", 2).unwrap().markdown, "第二版");

    // 回退到不存在的版本要报出来
    assert!(database
        .rollback("甲", 9, None)
        .unwrap_err()
        .contains("没有第 9 版"));

    cleanup(&database);
}

#[test]
fn a_single_revision_can_be_read_back() {
    let _guard = session_guard();
    let database = scratch("revision");
    session::forget_all();

    database.create("甲").unwrap();
    database.commit("甲", "第一版", None).unwrap();
    database.commit("甲", "第二版", None).unwrap();

    let first = database.read_revision("甲", 1).unwrap();
    assert_eq!(first.markdown, "第一版");
    assert_eq!(first.rev, 1);
    // 建立时间来自日志里的 Meta，看哪一版都一样
    assert_eq!(first.created, database.read("甲").unwrap().created);

    // 取不存在的版本要报出来，而不是给一份空的
    assert!(database
        .read_revision("甲", 9)
        .unwrap_err()
        .contains("没有第 9 版"));

    cleanup(&database);
}

#[test]
fn each_revision_reports_its_own_protection() {
    let _guard = session_guard();
    let database = scratch("revision-protection");
    session::forget_all();

    database.create("甲").unwrap();

    // 第一版明确用原样；第二版**显式换回压缩** —— 换保护是显式动作，两版封装因此不同
    database
        .commit_with("甲", "明文", None, Some(Policy::default()), None)
        .unwrap();
    database
        .commit_with(
            "甲",
            "压过的内容，写长一点好看出差别",
            None,
            Some(Policy {
                compress: true,
                ..Default::default()
            }),
            None,
        )
        .unwrap();

    // 各认各的：状态读的是**那一版自己的** blob 头
    assert!(database
        .read_revision("甲", 1)
        .unwrap()
        .protection
        .is_plain());
    assert!(database.read_revision("甲", 2).unwrap().protection.compress);

    cleanup(&database);
}

#[test]
fn the_history_lists_commits_only_and_newest_first() {
    let _guard = session_guard();
    let database = scratch("history");
    session::forget_all();

    database.create("甲").unwrap();
    database
        .commit("甲", "第一版", Some("开头".to_string()))
        .unwrap();
    database.commit("甲", "第二版", None).unwrap();
    // 草稿是槽位，不是一版 —— 它不该出现在历史里
    database.save_draft("甲", "还没定稿").unwrap();

    let history = database.revisions_of("甲").unwrap();
    assert_eq!(history.len(), 2, "草稿不该算一版：{history:?}");
    assert_eq!(history[0].rev, 2, "新的在前");
    assert_eq!(history[1].rev, 1);
    assert_eq!(history[1].summary.as_deref(), Some("开头"));
    assert!(history[0].at >= history[1].at);

    cleanup(&database);
}

#[test]
fn deleting_a_note_keeps_its_log() {
    let _guard = session_guard();
    let database = scratch("delete");
    session::forget_all();

    database.create("甲").unwrap();
    database.commit("甲", "内容", None).unwrap();
    database.save_draft("甲", "草稿").unwrap();

    let id = database.id_of("甲").unwrap();
    let log = database.log_path(&id);
    assert!(log.is_file());

    database.delete("甲").unwrap();

    // 列表里没了，也读不到了
    assert!(database.list().unwrap().is_empty());
    assert!(database.read("甲").is_err());
    assert!(database.id_of("甲").is_none());

    // 但日志还在 trash 里 —— 删错了捞得回来
    assert!(!log.is_file());
    assert!(database.trash_path(&id).is_file());
    // 草稿槽位也留着：标识不会重用，留着不碍事，还原时还写得上
    assert!(database.draft_path(&id).exists());

    // 再删一次要报"没有这篇"
    assert!(database.delete("甲").unwrap_err().contains("没有这篇"));

    cleanup(&database);
}
