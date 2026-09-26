//! 存储层的测试。
//!
//! 从 `mod.rs` 搬出来：那里原本四千多行，测试占了将近一半，把生产代码挤得看不清。
//! 子模块可以访问父模块的私有项，所以测试不必为可测性放宽任何可见性。

use super::*;
use std::sync::atomic::{AtomicU32, Ordering};

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// 测试用的临时仓库
struct TempVault {
    vault: Vault,
    root: PathBuf,
}

impl TempVault {
    fn new() -> Self {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("refind-vault-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let vault = Vault::open(&root).expect("open vault");
        Self { vault, root }
    }
}

impl Drop for TempVault {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn init_creates_expected_files() {
    let temp = TempVault::new();
    for dir in ["notes", "trash", "blobs"] {
        assert!(temp.root.join(dir).is_dir(), "{dir}/ 应当被建出来");
    }
    assert!(temp.root.join("vault.json").is_file());
    assert!(temp.root.join("namespaces.json").is_file());
    // 不再有全局索引：文件系统自己就是索引
    assert!(!temp.root.join("index.json").exists());
    assert!(temp.vault.list_notes().unwrap().is_empty());
}

#[test]
fn commit_then_load_roundtrip() {
    let temp = TempVault::new();
    let note = temp.vault.create("测试条目").unwrap();
    assert_eq!(note.rev, 0);
    assert_eq!(note.key, "0:测试条目");

    let note = temp
        .vault
        .commit("测试条目", "# 标题\n\n正文", Some("初稿"), 0)
        .unwrap();
    assert_eq!(note.rev, 1);
    assert!(note.html.contains("<h1"), "{}", note.html);
    assert_eq!(temp.vault.load("测试条目").unwrap().rev, 1);
}

#[test]
fn version_chain_keeps_parents_on_commits() {
    let temp = TempVault::new();
    temp.vault.create("链条").unwrap();
    temp.vault.commit("链条", "v1", None, 0).unwrap();
    temp.vault.commit("链条", "v2", None, 1).unwrap();
    temp.vault.commit("链条", "v3", None, 2).unwrap();
    assert_eq!(temp.vault.load("链条").unwrap().rev, 3);
    assert_eq!(temp.vault.load("链条").unwrap().markdown, "v3");
}

/// 文件名就是标题：列目录就能拿到全部笔记，不需要读文件、也不需要索引
#[test]
fn listing_comes_from_the_filesystem() {
    let temp = TempVault::new();
    temp.vault.create("甲").unwrap();
    temp.vault.create("乙").unwrap();
    temp.vault.commit("甲", "内容甲", None, 0).unwrap();

    let notes = temp.vault.list_notes().unwrap();
    assert_eq!(notes.len(), 2);

    // 改名之后列出来的必须是新名字（旧文件已经不在了）
    temp.vault.rename("甲", "丙").unwrap();
    let notes = temp.vault.list_notes().unwrap();
    let titles: Vec<&str> = notes.iter().map(|note| note.title.as_str()).collect();
    assert!(titles.contains(&"丙"), "{titles:?}");
    assert!(!titles.contains(&"甲"), "{titles:?}");
}

#[test]
fn rename_moves_the_file_and_keeps_the_history() {
    let temp = TempVault::new();
    temp.vault.create("旧名").unwrap();
    temp.vault.commit("旧名", "正文", None, 0).unwrap();

    let (_, before) = temp.vault.locate("旧名").unwrap();
    let note = temp.vault.rename("旧名", "新名").unwrap();

    assert_eq!(note.key, "0:新名");
    assert_eq!(note.rev, 2, "改名本身是一次提交");
    assert_eq!(note.markdown, "正文", "内容不变");
    // 改名**不搬文件**：同一个 id、同一个路径
    assert!(before.is_file(), "改名不该搬文件");
    let (_, after) = temp.vault.locate("新名").unwrap();
    assert_eq!(after, before, "改名前后路径应当一样");
    // 历史跟着文件走
    assert_eq!(temp.vault.history("新名").unwrap().len(), 3);
    assert!(matches!(
        temp.vault.load("旧名"),
        Err(VaultError::NotFound(_))
    ));
}

#[test]
fn delete_moves_the_file_to_trash() {
    let temp = TempVault::new();
    temp.vault.create("待删").unwrap();
    temp.vault.commit("待删", "正文", None, 0).unwrap();

    let (_, path) = temp.vault.locate("待删").unwrap();
    let id = Vault::id_from_path(&temp.vault.locate("待删").unwrap().1);
    temp.vault.delete("待删").unwrap();

    assert!(!path.is_file(), "笔记文件应当已经不在 notes/ 里");
    let trashed = temp.vault.trashed_path(&id);
    assert!(trashed.is_file(), "应当被挪进 trash/");

    // 删除标记还在：折叠出来是「已删除」
    let state = fold(&temp.vault.read_events_at(&trashed).unwrap());
    assert!(state.deleted);
    assert!(state.blob.is_some());

    // 而且能分辨「删过」和「从没建过」
    let outcome = temp.vault.load_outcome("待删").unwrap();
    assert!(outcome.note.is_none() && outcome.deleted);
    let outcome = temp.vault.load_outcome("从没建过").unwrap();
    assert!(outcome.note.is_none() && !outcome.deleted);
}

#[test]
fn draft_is_chained_but_superseded_by_commit() {
    let temp = TempVault::new();
    temp.vault.create("草稿").unwrap();
    temp.vault.commit("草稿", "v1", None, 0).unwrap();

    temp.vault.save_draft("草稿", "v1-草稿a", 1).unwrap();
    temp.vault.save_draft("草稿", "v1-草稿b", 1).unwrap();

    let draft = temp.vault.load_draft("草稿").unwrap().unwrap();
    assert_eq!(draft.markdown, "v1-草稿b");
    assert_eq!(draft.base_rev, 1);

    let note = temp
        .vault
        .commit("草稿", "v1-草稿b", Some("提交"), 1)
        .unwrap();
    assert_eq!(note.rev, 4, "版本号是一条序列，草稿也占号");
    assert!(temp.vault.load_draft("草稿").unwrap().is_none());

    let events = temp.vault.events_for("草稿").unwrap();
    let commit = events
        .iter()
        .find_map(|event| match event {
            Event::Rev {
                rev: 4,
                supersedes,
                parent,
                ..
            } => Some((supersedes.clone(), *parent)),
            _ => None,
        })
        .expect("找到提交 4");
    assert_eq!(commit.0, vec![2, 3], "本次提交取代了那两个草稿节点");
    assert_eq!(commit.1, Some(1), "parent 必须指向上一个提交，而不是草稿");
}

#[test]
fn prune_removes_superseded_drafts_without_breaking_the_chain() {
    let temp = TempVault::new();
    temp.vault.create("清理").unwrap();
    temp.vault.commit("清理", "v1", None, 0).unwrap();
    temp.vault.save_draft("清理", "draft-a", 1).unwrap();
    temp.vault.save_draft("清理", "draft-b", 1).unwrap();
    temp.vault.commit("清理", "v2", None, 1).unwrap();

    assert_eq!(temp.vault.prune("清理").unwrap(), 2);
    let note = temp.vault.load("清理").unwrap();
    assert_eq!(note.rev, 4);
    assert_eq!(note.markdown, "v2");
    assert!(!temp
        .vault
        .events_for("清理")
        .unwrap()
        .iter()
        .any(|event| matches!(event, Event::Auto { .. })));
}

#[test]
fn discard_draft_removes_only_the_current_spur() {
    let temp = TempVault::new();
    temp.vault.create("丢弃").unwrap();
    temp.vault.commit("丢弃", "v1", None, 0).unwrap();
    temp.vault.save_draft("丢弃", "draft", 1).unwrap();

    assert_eq!(temp.vault.discard_draft("丢弃").unwrap(), 1);
    assert!(temp.vault.load_draft("丢弃").unwrap().is_none());
    assert_eq!(temp.vault.load("丢弃").unwrap().markdown, "v1");
}

#[test]
fn identical_draft_is_not_appended_twice() {
    let temp = TempVault::new();
    temp.vault.create("幂等").unwrap();
    temp.vault.save_draft("幂等", "一样的内容", 0).unwrap();
    temp.vault.save_draft("幂等", "一样的内容", 0).unwrap();
    let autos = temp
        .vault
        .events_for("幂等")
        .unwrap()
        .iter()
        .filter(|event| matches!(event, Event::Auto { .. }))
        .count();
    assert_eq!(autos, 1, "内容没变不该重复追加");
}

#[test]
fn commit_with_stale_base_rev_is_rejected() {
    let temp = TempVault::new();
    temp.vault.create("冲突").unwrap();
    temp.vault.commit("冲突", "v1", None, 0).unwrap();
    assert!(matches!(
        temp.vault.commit("冲突", "v2", None, 0).unwrap_err(),
        VaultError::Conflict {
            expected: 0,
            found: 1
        }
    ));
}

#[test]
fn renaming_onto_an_existing_title_is_rejected() {
    let temp = TempVault::new();
    temp.vault.create("甲").unwrap();
    temp.vault.create("乙").unwrap();
    assert!(matches!(
        temp.vault.rename("甲", "乙").unwrap_err(),
        VaultError::NameTaken(_)
    ));
}

#[test]
fn red_and_blue_links_are_marked() {
    let temp = TempVault::new();
    temp.vault.create("目标条目").unwrap();
    temp.vault.commit("目标条目", "我是目标", None, 0).unwrap();
    temp.vault.create("来源").unwrap();
    let note = temp
        .vault
        .commit("来源", "[[目标条目|去看]] 和 [[不存在的条目]]", None, 0)
        .unwrap();
    assert!(note.html.contains(r#"data-key="0:目标条目""#), "{}", note.html);
    assert!(note.html.contains(r#"data-missing="false""#), "{}", note.html);
    assert!(note.html.contains(r#"data-missing="true""#), "{}", note.html);
}

#[test]
fn seed_runs_only_once() {
    let temp = TempVault::new();
    assert!(temp.vault.seed_if_empty("示例", "内容").unwrap());
    assert!(!temp.vault.seed_if_empty("示例", "内容").unwrap());
    assert_eq!(temp.vault.list_notes().unwrap().len(), 1);
}

#[test]
fn log_format_is_stable() {
    let temp = TempVault::new();
    temp.vault.seed_if_empty("格式", "内容").unwrap();
    temp.vault.save_draft("格式", "草稿", 1).unwrap();
    temp.vault.commit("格式", "定稿", Some("提交"), 1).unwrap();

    let id = temp.vault.note_ids().unwrap().remove(0);
    let log = temp
        .vault
        .read_events_at(&temp.vault.log_path(&id))
        .map(|events| {
            events
                .iter()
                .map(|event| serde_json::to_string(event).unwrap())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap();

    println!("---- notes/{id}.log ----\n{log}");
    let lines: Vec<&str> = log.lines().collect();
    assert_eq!(lines.len(), 4, "meta + rev1 + auto + rev2");
    assert!(lines[0].contains(r#""t":"meta""#), "{}", lines[0]);
    assert!(lines[1].contains(r#""t":"rev""#), "{}", lines[1]);
    assert!(lines[2].contains(r#""t":"auto""#), "{}", lines[2]);
    assert!(lines[2].contains(r#""on":1"#), "{}", lines[2]);
    assert!(lines[3].contains(r#""supersedes":[2]"#), "{}", lines[3]);
    assert!(lines[3].contains(r#""parent":1"#), "{}", lines[3]);
}

#[test]
fn opens_and_creates_a_missing_vault() {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let base = std::env::temp_dir().join(format!("refind-make-{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    let root = base.join("nested").join("deeper");

    assert!(!root.exists(), "前提：这个目录一开始不存在");
    let vault = Vault::open(&root).expect("应当自动建库");
    for dir in ["notes", "trash", "blobs"] {
        assert!(root.join(dir).is_dir(), "{dir}/ 应当被建出来");
    }
    assert!(root.join("vault.json").is_file());
    assert!(root.join("namespaces.json").is_file());
    assert!(vault.list_notes().unwrap().is_empty());

    println!("---- 自动建出来的仓库：{} ----", root.display());
    let mut entries: Vec<_> = fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    entries.sort();
    for path in entries {
        let kind = if path.is_dir() { "目录" } else { "文件" };
        println!("  {kind}  {}", path.file_name().unwrap().to_string_lossy());
    }

    let _ = fs::remove_dir_all(&base);
}

fn count_blobs(root: &std::path::Path) -> usize {
    let mut count = 0;
    if let Ok(prefixes) = fs::read_dir(root.join("blobs")) {
        for prefix in prefixes.flatten() {
            if let Ok(entries) = fs::read_dir(prefix.path()) {
                count += entries.flatten().count();
            }
        }
    }
    count
}

/// GC 的两个开关各自可控，而且只回收该回收的
#[test]
fn gc_switches_are_independent() {
    let temp = TempVault::new();
    temp.vault.create("回收").unwrap();
    temp.vault.commit("回收", "v1", None, 0).unwrap();
    temp.vault.save_draft("回收", "草稿甲", 1).unwrap();
    temp.vault.commit("回收", "v2", None, 1).unwrap();

    // 只开草稿那一项：blob 一个都不该动
    let before = count_blobs(&temp.root);
    let report = temp.vault.gc(false, true).unwrap();
    assert_eq!(report.removed_drafts, 1, "那条草稿已被提交取代");
    assert_eq!(report.removed_blobs, 0, "没开就不动 blob");
    assert_eq!(report.freed_bytes, 0);
    assert_eq!(count_blobs(&temp.root), before);

    // 再开 blob 那一项：草稿的正文已无引用，应当被回收
    let report = temp.vault.gc(true, false).unwrap();
    assert!(report.removed_blobs >= 1, "草稿正文已经没人引用了");
    assert!(report.freed_bytes > 0);
    assert_eq!(report.removed_drafts, 0, "没开就不动草稿");

    // 回收之后内容照样读得出来
    assert_eq!(temp.vault.load("回收").unwrap().markdown, "v2");
}

/// 被删除的笔记也要参与引用统计，否则它的正文会被误回收
#[test]
fn gc_keeps_blobs_referenced_by_trashed_notes() {
    let temp = TempVault::new();
    temp.vault.create("待删").unwrap();
    temp.vault.commit("待删", "要被保留的正文", None, 0).unwrap();
    temp.vault.delete("待删").unwrap();

    let report = temp.vault.gc(true, true).unwrap();
    assert_eq!(report.removed_blobs, 0, "trash 里的笔记还在引用它");

    let id = Vault::id_from_path(&temp.vault.locate("待删").unwrap().1);
    let state = fold(&temp.vault.read_events_at(&temp.vault.trashed_path(&id)).unwrap());
    let blob = state.blob.expect("应当还有内容引用");
    assert_eq!(
        String::from_utf8(temp.vault.blobs.get(&blob).unwrap()).unwrap(),
        "要被保留的正文"
    );
}

#[test]
fn validate_title_reports_the_same_rules_as_parsing() {
    let temp = TempVault::new();
    assert!(temp.vault.validate_title("合法标题").is_ok());
    assert!(temp.vault.validate_title("带@符号").is_err());
    assert!(temp.vault.validate_title("Help:目录").is_err(), "只有主命名空间");
    assert!(temp.vault.validate_title("").is_err());
    // 校验不该留下任何文件
    assert!(temp.root.join("notes").read_dir().unwrap().next().is_none());
}

/// 每个版本都有稳定 ID，数字与缩写两种引用都能解析
#[test]
fn revisions_have_short_ids_that_resolve() {
    let temp = TempVault::new();
    temp.vault.create("编号").unwrap();
    temp.vault.commit("编号", "第一版", None, 0).unwrap();
    temp.vault.commit("编号", "第二版", None, 1).unwrap();

    let history = temp.vault.history("编号").unwrap();
    let second = history.iter().find(|item| item.rev == 2).unwrap();
    assert_eq!(second.id.len(), 64, "完整 ID 是 sha256");
    assert_eq!(second.short_id.len(), 8);
    assert!(second.id.starts_with(&second.short_id));

    // ID 必须稳定：再查一次还是同一个
    let again = temp.vault.history("编号").unwrap();
    assert_eq!(
        again.iter().find(|item| item.rev == 2).unwrap().id,
        second.id
    );

    // 数字与缩写都能解析到同一个版本
    assert_eq!(temp.vault.resolve_revision("编号", "2").unwrap(), 2);
    assert_eq!(
        temp.vault.resolve_revision("编号", &second.short_id).unwrap(),
        2
    );
    assert_eq!(temp.vault.resolve_revision("编号", &second.id).unwrap(), 2);

    // 对不上的缩写要给「找不到」，而不是猜一个
    assert!(matches!(
        temp.vault.resolve_revision("编号", "zzzzzzzz"),
        Err(VaultError::RevisionNotFound { .. })
    ));
}

/// 地址栏解析：标题、标题@数字、标题@缩写、只写 @缩写、不存在、歧义


/// 缩写下限：太短要明确报错（纯数字版本号不受限）
#[test]
fn short_ids_have_a_minimum_length() {
    let temp = TempVault::new();
    temp.vault.create("下限").unwrap();
    temp.vault.commit("下限", "第一版", None, 0).unwrap();

    assert_eq!(temp.vault.resolve_revision("下限", "1").unwrap(), 1, "数字版本号不受限");

    let error = temp.vault.resolve_revision("下限", "ab").unwrap_err();
    assert!(
        matches!(error, VaultError::BadAddress(_)),
        "太短的缩写应当是提示，而不是「找不到」：{error}"
    );

    let history = temp.vault.history("下限").unwrap();
    let short = history.iter().find(|item| item.rev == 1).unwrap().short_id.clone();
    assert!(short.len() >= 5, "展示用的缩写本来就有 8 位");
    assert_eq!(temp.vault.resolve_revision("下限", &short).unwrap(), 1);
}

/// 草稿也带 ID，界面才能用「标题@缩写」预览它
#[test]
fn drafts_expose_their_own_id() {
    let temp = TempVault::new();
    temp.vault.create("草稿号").unwrap();
    temp.vault.commit("草稿号", "第一版", None, 0).unwrap();
    temp.vault.save_draft("草稿号", "草稿内容", 1).unwrap();

    let draft = temp.vault.load_draft("草稿号").unwrap().unwrap();
    assert_eq!(draft.id.len(), 64);
    assert!(draft.short_id.len() >= 5);

    // 用这个缩写能解析到草稿那一版，并读到它的内容
    let rev = temp.vault.resolve_revision("草稿号", &draft.short_id).unwrap();
    assert_eq!(rev, 2);
    assert_eq!(temp.vault.revision("草稿号", rev).unwrap().markdown, "草稿内容");
}

/// 回归：8 位十六进制缩写里约 2% 会**恰好全是数字**，
/// 以前先按「纯数字」判断，会把这种缩写误当版本号（偶发把 rev 解析成八位数）。
#[test]
fn all_digit_short_ids_still_resolve() {
    let temp = TempVault::new();
    temp.vault.create("回归").unwrap();

    let mut text = String::from("起始\n");
    temp.vault.commit("回归", &text, None, 0).unwrap();
    for round in 0..200 {
        text.push_str(&format!("第 {round} 行\n"));
        temp.vault
            .commit("回归", &text, None, (round + 1) as u64)
            .unwrap();
    }

    let history = temp.vault.history("回归").unwrap();
    assert_eq!(history.len(), 202, "1 条创建 + 201 次提交");

    // 每一版的缩写都必须解析回**它自己**（全数字的那些也一样）
    let mut all_digit = 0;
    for item in history.iter().filter(|item| item.rev > 0) {
        if item.short_id.chars().all(|ch| ch.is_ascii_digit()) {
            all_digit += 1;
        }
        assert_eq!(
            temp.vault.resolve_revision("回归", &item.short_id).unwrap(),
            item.rev,
            "缩写 {} 应当解析回版本 {}",
            item.short_id,
            item.rev
        );
    }
    println!("200 个缩写里有 {all_digit} 个恰好全是数字");
}

/// 碰撞防线：宁可拒绝写入，也不要留下两个同一个 ID 的版本
#[test]
fn id_collisions_are_rejected() {
    let temp = TempVault::new();
    temp.vault.create("撞车").unwrap();
    temp.vault.commit("撞车", "正文", None, 0).unwrap();

    let (_parsed, _) = temp.vault.locate("撞车").unwrap();
    let id = Vault::id_from_path(&temp.vault.locate("撞车").unwrap().1);
    let events = temp.vault.events_for("撞车").unwrap();
    let duplicate = events
        .iter()
        .find(|event| matches!(event, Event::Rev { .. }))
        .expect("有提交事件")
        .clone();

    // 把同一个事件再写一次：派生出的 ID 必然相同
    let error = temp.vault.append(&id, &duplicate).unwrap_err();
    assert!(
        matches!(error, VaultError::IdCollision(_)),
        "重复 ID 应当被拒绝：{error}"
    );

    // 拒绝之后日志没有被改动
    assert_eq!(
        temp.vault.read_events(&id).unwrap().len(),
        events.len(),
        "拒绝写入时不该落下任何东西"
    );
}

/// 模式也是地址语法的一部分，并且带回规范地址用于回显


/// 标准顺序 NAMESPACE:NAME@VERSION#SECTION$STATE：顺序不强制，回显一律标准


/// 规范化裁剪：某一版不能编辑，历史不属于某一版


/// 新语法：状态用 @，版本用 view- / rollback- 前缀（版本收起来，不再组合）


/// 页面地址模型：NAMESPACE:NAME@STATE，默认折叠成最简的 NAME
#[test]
fn address_model_collapses_to_the_simplest_form() {
    let temp = TempVault::new();
    temp.vault.create("模型").unwrap();
    temp.vault.commit("模型", "第一版", None, 0).unwrap();
    temp.vault.commit("模型", "第二版", None, 1).unwrap();

    let first = temp
        .vault
        .history("模型")
        .unwrap()
        .into_iter()
        .find(|item| item.rev == 1)
        .unwrap();

    // 默认（命名空间 0 + 看最新提交）→ 最简形式
    match temp.vault.parse_address("模型").unwrap() {
        Address::Note { title, address, .. } => {
            assert_eq!(title, "模型");
            assert_eq!(address, "模型");
        }
        other => panic!("{other:?}"),
    }

    // 其余状态
    match temp.vault.parse_address("模型@edit").unwrap() {
        Address::Edit { address, .. } => assert_eq!(address, "模型@edit"),
        other => panic!("{other:?}"),
    }
    match temp.vault.parse_address("模型@history").unwrap() {
        Address::History { address, .. } => assert_eq!(address, "模型@history"),
        other => panic!("{other:?}"),
    }
    match temp.vault.parse_address("模型@delete").unwrap() {
        Address::Delete { address, .. } => assert_eq!(address, "模型@delete"),
        other => panic!("{other:?}"),
    }

    // 状态与章节一起
    match temp.vault.parse_address("模型@history#小节").unwrap() {
        Address::History { address, .. } => assert_eq!(address, "模型@history#小节"),
        other => panic!("{other:?}"),
    }

    // view-版本：缩写与数字版本号都行，**回显一律缩写 + 完整状态**
    match temp
        .vault
        .parse_address(&format!("模型@view-{}", first.short_id))
        .unwrap()
    {
        Address::ViewVersion {
            title,
            rev,
            short_id,
            address,
            ..
        } => {
            assert_eq!(title, "模型");
            assert_eq!(rev, 1);
            assert_eq!(short_id, first.short_id);
            assert_eq!(address, format!("模型@view-{}", first.short_id));
        }
        other => panic!("{other:?}"),
    }
    match temp.vault.parse_address("模型@view-1").unwrap() {
        Address::ViewVersion { rev, address, .. } => {
            assert_eq!(rev, 1);
            assert_eq!(address, format!("模型@view-{}", first.short_id));
        }
        other => panic!("{other:?}"),
    }

    // rollback-版本
    match temp.vault.parse_address("模型@rollback-1").unwrap() {
        Address::RollbackConfirm { rev, address, .. } => {
            assert_eq!(rev, 1);
            assert_eq!(address, format!("模型@rollback-{}", first.short_id));
        }
        other => panic!("{other:?}"),
    }

    // 状态写错要报错；回退不指出哪一版也报错
    assert!(temp.vault.parse_address("模型@whatever").is_err());
    assert!(temp.vault.parse_address("模型@rollback").is_err());
    // 版本对不上是错误
    assert!(temp.vault.parse_address("模型@view-abcde").is_err());
}

/// 端到端：界面按地址驱动的三条流程 —— 看某一版、回退确认、删除确认
#[test]
fn address_driven_flows_end_to_end() {
    let temp = TempVault::new();
    temp.vault.create("流程").unwrap();
    temp.vault.commit("流程", "第一版", None, 0).unwrap();
    temp.vault.commit("流程", "第二版", None, 1).unwrap();

    // 看某一版：地址给出 rev，界面据此取内容
    let rev = match temp.vault.parse_address("流程@view-1").unwrap() {
        Address::ViewVersion { rev, .. } => rev,
        other => panic!("{other:?}"),
    };
    assert_eq!(rev, 1);
    assert_eq!(temp.vault.revision("流程", rev).unwrap().markdown, "第一版");

    // 回退确认页 → 用户确认 → 真的回退（界面调 revert_note，这里是它的等价动作）
    let (title, rev) = match temp.vault.parse_address("流程@rollback-1").unwrap() {
        Address::RollbackConfirm { title, rev, .. } => (title, rev),
        other => panic!("{other:?}"),
    };
    let old = temp.vault.revision(&title, rev).unwrap().markdown;
    temp.vault.commit(&title, &old, Some("回退"), 2).unwrap();
    assert_eq!(temp.vault.load("流程").unwrap().markdown, "第一版");
    assert_eq!(temp.vault.load("流程").unwrap().rev, 3, "回退是一次新提交");

    // 删除确认页 → 用户确认 → 文件进 trash/，再解析同一地址就是「不存在」
    match temp.vault.parse_address("流程@delete").unwrap() {
        Address::Delete { title, .. } => temp.vault.delete(&title).unwrap(),
        other => panic!("{other:?}"),
    }
    match temp.vault.parse_address("流程@delete").unwrap() {
        Address::Missing { title, .. } => assert_eq!(title, "流程"),
        other => panic!("{other:?}"),
    }
}

/// 虚拟命名空间 special:：不对应笔记，交给前端渲染；大小写不敏感
#[test]
fn special_namespace_does_not_map_to_a_note() {
    let temp = TempVault::new();

    match temp.vault.parse_address("special:newtab").unwrap() {
        Address::Special { page, address, .. } => {
            assert_eq!(page, "newtab");
            assert_eq!(address, "special:newtab");
        }
        other => panic!("{other:?}"),
    }

    // 大小写不敏感，回显一律小写
    match temp.vault.parse_address("Special:NewTab").unwrap() {
        Address::Special { page, address, .. } => {
            assert_eq!(page, "newtab");
            assert_eq!(address, "special:newtab");
        }
        other => panic!("{other:?}"),
    }

    // 章节跟着特殊页面走（前端自己处理），不进 page 名
    match temp.vault.parse_address("special:newtab#小节").unwrap() {
        Address::Special { page, .. } => assert_eq!(page, "newtab"),
        other => panic!("{other:?}"),
    }

    // 空页面名要报错
    assert!(temp.vault.parse_address("special:").is_err());

    // 笔记不可能叫这个名字：标题里禁止冒号
    assert!(temp.vault.validate_title("special:newtab").is_err());
}

/// 回归：多字节标题不能让解析 panic。
///
/// 曾经写成 `raw[..8]`（按字节切），中文标题会让切口落在字符中间 —— 而且这个
/// panic 在 GTK 回调里不能 unwind，会把整个应用 abort。
#[test]
fn multi_byte_titles_do_not_panic() {
    let temp = TempVault::new();

    // 「平陆运河」是 12 字节，第 8 个字节落在「运」中间
    assert!(matches!(
        temp.vault.parse_address("平陆运河").unwrap(),
        Address::Missing { .. }
    ));

    // 各种长度都过一遍，确保没有别处按字节切
    for title in ["页", "页面", "页面名", "页面名字", "页面名字啊", "页面名字啊啊"] {
        let _ = temp.vault.parse_address(title);
        let _ = temp.vault.validate_title(title);
    }

    // 前缀判断本身也要能处理多字节（这里曾按字节切，直接 panic）。
    // 现在前缀解析走命名空间表，所以用**行为**来断言，而不是测某个内部函数。
    assert!(
        matches!(
            temp.vault.parse_address("Special:newtab").unwrap(),
            Address::Special { .. }
        ),
        "特殊页面前缀大小写不敏感"
    );
    assert!(
        temp.vault.parse_address("特殊:newtab").is_err(),
        "没登记过的前缀不是命名空间"
    );
}

/// 数据模型：**文件名是纯 ASCII id，标题只存在 titles.json 里**
#[test]
fn file_names_are_hex_ids_and_titles_live_in_json() {
    let temp = TempVault::new();
    temp.vault.create("标题不进文件名").unwrap();
    temp.vault
        .commit("标题不进文件名", "正文", None, 0)
        .unwrap();

    let ids = temp.vault.note_ids().unwrap();
    assert_eq!(ids.len(), 1);
    let id = ids[0].clone();
    assert!(
        id.chars().all(|ch| ch.is_ascii_hexdigit()),
        "文件名应当是纯十六进制：{id}"
    );
    assert!(
        temp.root
            .join("notes")
            .join("0")
            .join(format!("{id}.log"))
            .is_file(),
        "内容应当在 notes/0/<id>.log"
    );

    let table = fs::read_to_string(temp.root.join("titles.json")).unwrap();
    assert!(
        table.contains("标题不进文件名"),
        "名字应当存在 titles.json 里：{table}"
    );

    // 改名：文件跟着 id 走（换到新标题的 id），名字表里换一行，旧名字不留
    temp.vault.rename("标题不进文件名", "换个名字").unwrap();
    assert_eq!(temp.vault.note_ids().unwrap().len(), 1, "改名不该留下旧文件");
    let table = fs::read_to_string(temp.root.join("titles.json")).unwrap();
    assert!(table.contains("换个名字"), "{table}");
    assert!(!table.contains("标题不进文件名"), "{table}");

    // 删除：名字从 notes 挪到 trashed，文件进 trash/
    temp.vault.delete("换个名字").unwrap();
    let table = fs::read_to_string(temp.root.join("titles.json")).unwrap();
    assert!(table.contains("trashed"), "{table}");
    assert!(table.contains("换个名字"), "删除后名字要留在 trashed 里：{table}");
}

/// 状态与章节随便怎么排，回显一律标准顺序；看最新提交时 `view-` 会被裁掉
#[test]
fn address_is_normalized_and_latest_view_is_cropped() {
    let temp = TempVault::new();
    temp.vault.create("顺序").unwrap();
    temp.vault.commit("顺序", "第一版", None, 0).unwrap();
    temp.vault.commit("顺序", "第二版", None, 1).unwrap();

    let history = temp.vault.history("顺序").unwrap();
    let first = history.iter().find(|item| item.rev == 1).unwrap();
    let latest = history.iter().find(|item| item.rev == 2).unwrap();

    // 顺序乱写（章节在前、状态在后）→ 回显按 `NAME@STATE#SECTION`
    match temp
        .vault
        .parse_address(&format!("顺序#小节@view-{}", first.short_id))
        .unwrap()
    {
        Address::ViewVersion { address, rev, .. } => {
            assert_eq!(rev, 1);
            assert_eq!(address, format!("顺序@view-{}#小节", first.short_id));
        }
        other => panic!("{other:?}"),
    }

    // 看的就是最新提交 → 裁掉 `view-`
    match temp
        .vault
        .parse_address(&format!("顺序@view-{}", latest.short_id))
        .unwrap()
    {
        Address::Note { address, title, .. } => {
            assert_eq!(title, "顺序");
            assert_eq!(address, "顺序");
        }
        other => panic!("{other:?}"),
    }

    // 带章节时：裁状态、留章节
    match temp
        .vault
        .parse_address(&format!("顺序@view-{}#小节", latest.short_id))
        .unwrap()
    {
        Address::Note { address, .. } => assert_eq!(address, "顺序#小节"),
        other => panic!("{other:?}"),
    }

    // rollback 不是默认状态，不能裁
    match temp
        .vault
        .parse_address(&format!("顺序@rollback-{}", latest.short_id))
        .unwrap()
    {
        Address::RollbackConfirm { address, .. } => {
            assert_eq!(address, format!("顺序@rollback-{}", latest.short_id));
        }
        other => panic!("{other:?}"),
    }
}

/// 特殊页面：不存在要报错，状态不正确要裁掉（章节与状态都按标准顺序回显）
#[test]
fn special_pages_validate_and_crop_state() {
    let temp = TempVault::new();

    // 状态不合法 → 裁掉，仍然打开那个页面（不当错误）
    match temp.vault.parse_address("special:newtab@edit").unwrap() {
        Address::Special { page, address, .. } => {
            assert_eq!(page, "newtab");
            assert_eq!(address, "special:newtab");
        }
        other => panic!("{other:?}"),
    }
    match temp.vault.parse_address("special:newtab@whatever").unwrap() {
        Address::Special { address, .. } => assert_eq!(address, "special:newtab"),
        other => panic!("{other:?}"),
    }

    // 章节保留
    match temp.vault.parse_address("special:newtab#小节").unwrap() {
        Address::Special { address, .. } => assert_eq!(address, "special:newtab#小节"),
        other => panic!("{other:?}"),
    }

    // 不存在的特殊页面 → 报错，且提示里带上现有的页面
    let error = temp.vault.parse_address("special:不存在").unwrap_err();
    let text = error.to_string();
    assert!(text.contains("没有这个特殊页面"), "{text}");
    assert!(text.contains("newtab"), "{text}");

    // 新增的页面同样注册在册（special:all）
    match temp.vault.parse_address("special:all").unwrap() {
        Address::Special { page, address, .. } => {
            assert_eq!(page, "all");
            assert_eq!(address, "special:all");
        }
        other => panic!("{other:?}"),
    }

    // 空页面名仍然是另一种错误
    assert!(temp.vault.parse_address("special:").is_err());
}

/// 界面偏好住在 preferences.json，且**不会**把仓库设置文件写脏
#[test]
fn appearance_lives_in_its_own_file() {
    let mut temp = TempVault::new();
    let before = fs::read_to_string(temp.root.join("vault.json")).unwrap();

    temp.vault
        .update_settings(
            None,
            None,
            None,
            None,
            None,
            Some("light".to_string()),
            Some("#123456".to_string()),
            Some(900),
            None,
        )
        .unwrap();

    let preferences = fs::read_to_string(temp.root.join("preferences.json")).unwrap();
    assert!(preferences.contains("\"light\""), "{preferences}");
    assert!(preferences.contains("#123456"), "{preferences}");
    assert!(preferences.contains("900"), "{preferences}");

    let after = fs::read_to_string(temp.root.join("vault.json")).unwrap();
    assert_eq!(before, after, "改外观不该动 vault.json");
    assert!(!after.contains("appearance"), "vault.json 里不该再有 appearance：{after}");

    assert_eq!(temp.vault.preferences().theme, "light");
    assert_eq!(temp.vault.settings_view().accent, "#123456");
}

/// 引用解析不出来时，提示里不能出现"没有版本 0"
/// （0 是"给不出版本号"的内部占位，不是真实版本）
#[test]
fn unresolvable_reference_does_not_report_version_zero() {
    let temp = TempVault::new();
    temp.vault.create("引用").unwrap();
    temp.vault.commit("引用", "正文", None, 0).unwrap();

    // 5 位以上、既不是任何 commit 的前缀，也不是数字
    let error = temp.vault.resolve_revision("引用", "zzzzz").unwrap_err();
    let text = error.to_string();
    assert!(!text.contains("版本 0"), "不该打出占位用的 0：{text}");
    assert!(text.contains("没有这个版本"), "{text}");
}

/// 指令页面：第一行 `$$COMMAND$$`（忽略末尾空白）+ 第二行 `REDIRECT: 地址`
#[test]
fn command_page_redirects_and_no_command_shows_itself() {
    let temp = TempVault::new();
    temp.vault.create("目标").unwrap();
    temp.vault.commit("目标", "正文", None, 0).unwrap();

    // 第一行末尾故意留空格：规则是"忽略末尾空白"
    temp.vault.create("指令页").unwrap();
    temp.vault
        .commit("指令页", "$$COMMAND$$   \nREDIRECT: 目标\n", None, 0)
        .unwrap();

    // 不带状态：跟重定向，落到目标上（回显也是目标）
    match temp.vault.parse_address("指令页").unwrap() {
        Address::Note {
            title,
            address,
            code_block,
            via,
        } => {
            assert_eq!(title, "目标");
            assert_eq!(address, "目标");
            assert!(!code_block);
            // 跟重定向来的要带上"从哪儿来"：标题下方据此提示
            let via = via.expect("跟重定向来的应当有来源");
            assert_eq!(via.from, "指令页");
            assert!(!via.random, "这是重定向，不是随机跳转");
        }
        other => panic!("{other:?}"),
    }

    // @no-command：不跟重定向，显示它自己，回显保留状态
    match temp.vault.parse_address("指令页@no-command").unwrap() {
        Address::Note {
            title,
            address,
            code_block,
            via,
        } => {
            assert_eq!(title, "指令页");
            assert_eq!(address, "指令页@no-command");
            assert!(code_block);
            assert!(via.is_none(), "直接打开 @no-command 没有来源");
        }
        other => panic!("{other:?}"),
    }

    // 一般页面上的 @no-command：没有影响，回显裁掉
    match temp.vault.parse_address("目标@no-command").unwrap() {
        Address::Note {
            address, code_block, ..
        } => {
            assert_eq!(address, "目标");
            assert!(!code_block);
        }
        other => panic!("{other:?}"),
    }

    // @no-command 读出来的 html 是代码块，markdown 保持原样（编辑器里仍看到原文）
    let outcome = temp.vault.load_code_blocked("指令页").unwrap();
    let note = outcome.note.unwrap();
    assert!(note.html.contains("<pre"), "{}", note.html);
    assert!(note.html.contains("<code"), "{}", note.html);
    assert_eq!(note.markdown, "$$COMMAND$$   \nREDIRECT: 目标\n");
}

/// 指令页面认不出指令时**必须报错**，不能当普通页面读 ——
/// 否则一条写坏的指令会静静显示成正文，谁也不知道它没生效。
#[test]
fn unrecognized_command_is_an_error() {
    let temp = TempVault::new();

    // 只有标记、没有第二行
    temp.vault.create("空指令").unwrap();
    temp.vault
        .commit("空指令", "$$COMMAND$$\n", None, 0)
        .unwrap();
    let error = temp.vault.parse_address("空指令").unwrap_err();
    assert!(error.to_string().contains("没写指令"), "{error}");

    // 有第二行，但不是 REDIRECT
    temp.vault.create("写错了").unwrap();
    temp.vault
        .commit("写错了", "$$COMMAND$$\nREDIRECTX: 目标\n", None, 0)
        .unwrap();
    let error = temp.vault.parse_address("写错了").unwrap_err();
    let text = error.to_string();
    assert!(text.contains("认不出来"), "{text}");
    assert!(text.contains("REDIRECTX"), "报错要带出写坏的那一行：{text}");

    // 逃生口：两者都能用 @no-command 进去看和改
    match temp.vault.parse_address("空指令@no-command").unwrap() {
        Address::Note { code_block, .. } => assert!(code_block),
        other => panic!("{other:?}"),
    }
    match temp.vault.parse_address("写错了@no-command").unwrap() {
        Address::Note { code_block, .. } => assert!(code_block),
        other => panic!("{other:?}"),
    }
}

/// 重定向成环要报错，而不是无限递归
#[test]
fn redirect_loop_is_reported() {
    let temp = TempVault::new();
    temp.vault.create("甲").unwrap();
    temp.vault
        .commit("甲", "$$COMMAND$$\nREDIRECT: 乙\n", None, 0)
        .unwrap();
    temp.vault.create("乙").unwrap();
    temp.vault
        .commit("乙", "$$COMMAND$$\nREDIRECT: 甲\n", None, 0)
        .unwrap();

    let error = temp.vault.parse_address("甲").unwrap_err();
    assert!(error.to_string().contains("重定向"), "{error}");
}

/// REDIRECT 没写目标 → 明确报错；而 @no-command 仍是逃生口，能进去看/改
#[test]
fn redirect_without_target_is_an_error() {
    let temp = TempVault::new();
    temp.vault.create("缺目标").unwrap();
    temp.vault
        .commit("缺目标", "$$COMMAND$$\nREDIRECT:\n", None, 0)
        .unwrap();

    assert!(temp.vault.parse_address("缺目标").is_err());
    match temp.vault.parse_address("缺目标@no-command").unwrap() {
        Address::Note { code_block, .. } => assert!(code_block),
        other => panic!("{other:?}"),
    }
}

/// 第一行不是标记时，即使第二行写了 REDIRECT 也不当指令页面
#[test]
fn redirect_needs_the_marker_on_the_first_line() {
    let temp = TempVault::new();
    temp.vault.create("目标2").unwrap();
    temp.vault.commit("目标2", "正文", None, 0).unwrap();
    temp.vault.create("伪装").unwrap();
    temp.vault
        .commit("伪装", "前言\nREDIRECT: 目标2\n", None, 0)
        .unwrap();

    match temp.vault.parse_address("伪装").unwrap() {
        Address::Note { address, .. } => assert_eq!(address, "伪装"),
        other => panic!("{other:?}"),
    }
}

/// `@view-<旧版>` 等同于启用 `@no-command`：看旧版时**不执行指令**，只把当时的原文
/// 包成代码块显示。一般页面的版本视图不受影响。
#[test]
fn version_view_of_a_command_page_shows_code() {
    let temp = TempVault::new();
    temp.vault.create("目标").unwrap();
    temp.vault.commit("目标", "正文", None, 0).unwrap();
    temp.vault.create("指令").unwrap();
    temp.vault
        .commit("指令", "$$COMMAND$$\nREDIRECT: 目标\n", None, 0)
        .unwrap();

    // 直接看：跟重定向
    match temp.vault.parse_address("指令").unwrap() {
        Address::Note { title, .. } => assert_eq!(title, "目标"),
        other => panic!("{other:?}"),
    }

    // 看这一版：不跟，包成代码块；markdown 保持原文
    let content = temp.vault.revision_code_blocked("指令", 1).unwrap();
    assert!(content.html.contains("<pre"), "{}", content.html);
    assert_eq!(content.markdown, "$$COMMAND$$\nREDIRECT: 目标\n");

    // 一般页面的版本视图不该被包成代码块
    let plain = temp.vault.revision_code_blocked("目标", 1).unwrap();
    assert!(!plain.html.contains("<pre"), "{}", plain.html);
}

/// RANDOM_REDIRECT：在命名空间里随机挑一篇，并**排除自己**
#[test]
fn random_redirect_picks_another_page() {
    let temp = TempVault::new();
    temp.vault.create("唯一候选").unwrap();
    temp.vault.commit("唯一候选", "正文", None, 0).unwrap();
    temp.vault.create("掷骰子").unwrap();
    temp.vault
        .commit("掷骰子", "$$COMMAND$$\nRANDOM_REDIRECT\n", None, 0)
        .unwrap();

    // 候选只有一篇，所以结果必然确定（顺带证明"排除自己"生效：
    // 否则可能随机到自己，一路跟到跳数上限）
    match temp.vault.parse_address("掷骰子").unwrap() {
        Address::Note { title, .. } => assert_eq!(title, "唯一候选"),
        other => panic!("{other:?}"),
    }
}

/// 命名空间参数：写 ID 与"冒号后空着"都合法（空 = 主命名空间）
#[test]
fn random_redirect_accepts_namespace_argument() {
    let with_id = TempVault::new();
    with_id.vault.create("候选").unwrap();
    with_id.vault.commit("候选", "正文", None, 0).unwrap();
    with_id.vault.create("带ID").unwrap();
    with_id
        .vault
        .commit("带ID", "$$COMMAND$$\nRANDOM_REDIRECT: 0\n", None, 0)
        .unwrap();
    match with_id.vault.parse_address("带ID").unwrap() {
        Address::Note { title, .. } => assert_eq!(title, "候选"),
        other => panic!("{other:?}"),
    }

    let empty_arg = TempVault::new();
    empty_arg.vault.create("候选").unwrap();
    empty_arg.vault.commit("候选", "正文", None, 0).unwrap();
    empty_arg.vault.create("空参数").unwrap();
    empty_arg
        .vault
        .commit("空参数", "$$COMMAND$$\nRANDOM_REDIRECT: \n", None, 0)
        .unwrap();
    match empty_arg.vault.parse_address("空参数").unwrap() {
        Address::Note { title, .. } => assert_eq!(title, "候选"),
        other => panic!("{other:?}"),
    }
}

/// RANDOM_REDIRECT 的三种失败都要说清楚
#[test]
fn random_redirect_errors_are_clear() {
    // 唯一一页就是它自己 → 排除自己后没有候选
    let lonely = TempVault::new();
    lonely.vault.create("孤零零").unwrap();
    lonely
        .vault
        .commit("孤零零", "$$COMMAND$$\nRANDOM_REDIRECT\n", None, 0)
        .unwrap();
    let error = lonely.vault.parse_address("孤零零").unwrap_err();
    assert!(error.to_string().contains("随机不到"), "{error}");

    // 命名空间写成**字符串标识**：`special` 是虚拟命名空间，页面由程序提供
    let special = TempVault::new();
    special.vault.create("随便一篇").unwrap();
    special.vault.commit("随便一篇", "正文", None, 0).unwrap();
    special.vault.create("跳特殊页").unwrap();
    special
        .vault
        .commit("跳特殊页", "$$COMMAND$$\nRANDOM_REDIRECT: special\n", None, 0)
        .unwrap();
    match special.vault.parse_address("跳特殊页").unwrap() {
        Address::Special { page, .. } => assert!(
            SPECIAL_PAGES.contains(&page.as_str()),
            "应当落到真实存在的特殊页面：{page}"
        ),
        // 也可能正好抽中 `special:random` —— 它自己还会再跳一次，于是最终落在
        // 某一篇笔记上。这是正确行为（它就是"随机"），不是失败。
        Address::Note { .. } => {}
        other => panic!("{other:?}"),
    }

    // 命名空间里没有笔记
    let empty_ns = TempVault::new();
    empty_ns.vault.create("候选").unwrap();
    empty_ns.vault.commit("候选", "正文", None, 0).unwrap();
    empty_ns.vault.create("别的空间").unwrap();
    empty_ns
        .vault
        .commit("别的空间", "$$COMMAND$$\nRANDOM_REDIRECT: 7\n", None, 0)
        .unwrap();
    let error = empty_ns.vault.parse_address("别的空间").unwrap_err();
    assert!(error.to_string().contains("随机不到"), "{error}");
}

/// `special:random` 等同于随机重定向：落到主命名空间的某一篇
#[test]
fn special_random_jumps_to_a_page() {
    let temp = TempVault::new();
    temp.vault.create("唯一页").unwrap();
    temp.vault.commit("唯一页", "正文", None, 0).unwrap();

    match temp.vault.parse_address("special:random").unwrap() {
        Address::Note { title, .. } => assert_eq!(title, "唯一页"),
        other => panic!("{other:?}"),
    }

    // 一篇都没有 → 明确报错，而不是给一页空白
    let empty = TempVault::new();
    let error = empty.vault.parse_address("special:random").unwrap_err();
    assert!(error.to_string().contains("随机不到"), "{error}");
}

/// `special:all` 的列表要能看出哪些是指令页面（含认不出的那种）
#[test]
fn listing_marks_command_pages() {
    let temp = TempVault::new();
    temp.vault.create("目标").unwrap();
    temp.vault.commit("目标", "正文", None, 0).unwrap();
    temp.vault.create("重定向页").unwrap();
    temp.vault
        .commit("重定向页", "$$COMMAND$$\nREDIRECT: 目标\n", None, 0)
        .unwrap();
    temp.vault.create("随机页").unwrap();
    temp.vault
        .commit("随机页", "$$COMMAND$$\nRANDOM_REDIRECT\n", None, 0)
        .unwrap();
    temp.vault.create("坏指令").unwrap();
    temp.vault
        .commit("坏指令", "$$COMMAND$$\n不知道写什么\n", None, 0)
        .unwrap();

    let listed = temp.vault.list_notes().unwrap();
    let info_of = |title: &str| {
        listed
            .iter()
            .find(|item| item.title == title)
            .unwrap_or_else(|| panic!("列表里没有《{title}》"))
            .command
            .clone()
    };

    assert!(info_of("目标").is_none(), "普通页面没有指令信息");

    // 短名、中文名、说明**全部来自后端那张指令表** —— 前端不再自己维护一份清单
    let redirect = info_of("重定向页").expect("重定向页应当有指令信息");
    assert_eq!(redirect.kind, "redirect");
    assert_eq!(redirect.label, "重定向");
    assert!(redirect.detail.contains("跳到"), "{}", redirect.detail);
    // 目标单独给出来，界面据此渲染成可点的链接（不再写进说明、也不再用书名号）
    assert_eq!(redirect.argument, "目标");

    assert_eq!(info_of("随机页").unwrap().kind, "random-redirect");

    // 认不出的那种最需要被看见：一打开就报错，得先在列表里找到它
    let broken = info_of("坏指令").unwrap();
    assert_eq!(broken.kind, "unrecognized");
    assert_eq!(broken.label, "指令有问题");
}

/// 回收站清单：删过的笔记按删除时间倒序列出，并带上"删了多久"
#[test]
fn trash_listing_reports_age() {
    let temp = TempVault::new();
    temp.vault.create("留下的").unwrap();
    temp.vault.commit("留下的", "正文", None, 0).unwrap();
    temp.vault.create("删掉的").unwrap();
    temp.vault.commit("删掉的", "被删的正文", None, 0).unwrap();
    temp.vault.delete("删掉的").unwrap();

    let listed = temp.vault.list_trash().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].title, "删掉的");
    assert_eq!(listed[0].days_old, Some(0), "刚删的应当是 0 天");
    assert!(listed[0].bytes > 0);
    assert!(!listed[0].deleted_at.is_empty());
}

/// 清理回收站：30 天不动刚删的；传 0 则全清，并顺手回收内容块
#[test]
fn purge_trash_respects_age_then_reclaims_blobs() {
    let temp = TempVault::new();
    temp.vault.create("留下的").unwrap();
    temp.vault.commit("留下的", "留下的正文", None, 0).unwrap();
    temp.vault.create("删掉的").unwrap();
    temp.vault
        .commit("删掉的", "只此一份的正文", None, 0)
        .unwrap();
    temp.vault.delete("删掉的").unwrap();

    // 30 天：刚删的不该被动
    let report = temp.vault.purge_trash(30).unwrap();
    assert_eq!(report.removed, 0);
    assert_eq!(temp.vault.list_trash().unwrap().len(), 1, "刚删的必须留着");

    // 0 天：全部清掉；那份正文只被这一页引用，所以内容块这时才成为孤块
    let report = temp.vault.purge_trash(0).unwrap();
    assert_eq!(report.removed, 1);
    assert!(temp.vault.list_trash().unwrap().is_empty());
    assert_eq!(
        report.blobs.removed_blobs, 1,
        "回收站日志没了，那份正文才无人引用"
    );
    assert!(temp.vault.load("留下的").is_ok(), "没删的那篇不受影响");
}

/// 回收站里的引用要**连同草稿节点**一起计入 blob 统计。
///
/// 已有的用例只覆盖了提交版本（`Rev`）；这里补上草稿（`Auto`）那一半 ——
/// 只被草稿引用的内容块，同样不能因为"引用它的笔记被删了"就被回收。
#[test]
fn gc_counts_draft_references_from_the_trash() {
    let temp = TempVault::new();
    temp.vault.create("带草稿").unwrap();
    temp.vault.commit("带草稿", "第一版", None, 0).unwrap();
    // 一份只属于草稿的正文
    temp.vault.save_draft("带草稿", "草稿独有的正文", 1).unwrap();
    temp.vault.delete("带草稿").unwrap();

    let report = temp.vault.gc(true, false).unwrap();
    assert_eq!(report.removed_blobs, 0, "回收站里的草稿仍然引用着它的内容块");

    // 整条清掉回收站之后，这块才真正无人引用
    let purged = temp.vault.purge_trash(0).unwrap();
    assert_eq!(purged.removed, 1);
    assert!(
        purged.blobs.removed_blobs >= 1,
        "回收站清掉后，草稿独有的内容块才成为孤块"
    );
}

/// "到点了吗"：刚跑过就不是，间隔过去或从没跑过就是
#[test]
fn maintenance_pending_follows_the_last_run() {
    let fresh = TempVault::new();
    assert!(fresh.vault.maintenance_pending(), "从没跑过就该跑");

    let mut ran = TempVault::new();
    ran.vault.run_maintenance().unwrap();
    assert!(
        !ran.vault.maintenance_pending(),
        "刚跑过、间隔没到，就不该再建任务"
    );
}

/// 还原：文件搬回来、名字挪回去，并**追加一版**（历史一条不丢）
#[test]
fn restore_brings_a_note_back() {
    let temp = TempVault::new();
    temp.vault.create("回来的").unwrap();
    temp.vault.commit("回来的", "正文还在", None, 0).unwrap();
    temp.vault.delete("回来的").unwrap();
    assert!(temp.vault.list_trash().unwrap().len() == 1);

    let note = temp.vault.restore_note("回来的").unwrap();
    assert_eq!(note.markdown, "正文还在", "内容原样回来");
    assert!(temp.vault.list_trash().unwrap().is_empty());
    assert!(
        temp.vault.list_notes().unwrap().iter().any(|item| item.title == "回来的"),
        "应当重新出现在笔记列表里"
    );
    assert!(temp.vault.load("回来的").is_ok());

    // 历史一条不丢：创建 / 提交 / 删除 / 还原
    let history = temp.vault.history("回来的").unwrap();
    assert!(
        history.iter().any(|item| item.summary.as_deref() == Some("从回收站还原")),
        "还原本身应当在历史里留下一笔"
    );
    assert!(history.len() >= 4, "{history:?}");
}

/// 立即清除：只清一条，别的还在；且**不**顺手回收内容块（那件事交给数据库回收页）
#[test]
fn purge_entry_removes_one() {
    let temp = TempVault::new();
    for title in ["留着", "马上清"] {
        temp.vault.create(title).unwrap();
        temp.vault.commit(title, "各自的内容", None, 0).unwrap();
        temp.vault.delete(title).unwrap();
    }

    temp.vault.purge_trash_entry("马上清").unwrap();
    let listed = temp.vault.list_trash().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].title, "留着");

    assert!(temp.vault.restore_note("留着").is_ok(), "别的那条仍可还原");
    assert!(temp.vault.purge_trash_entry("马上清").is_err(), "已经清了");
}

/// 自动维护：**按上次执行时间判定**，间隔之内什么都不做
#[test]
fn maintenance_runs_once_per_interval() {
    let mut temp = TempVault::new();
    temp.vault.create("甲").unwrap();
    temp.vault.commit("甲", "正文", None, 0).unwrap();
    temp.vault.create("乙").unwrap();
    temp.vault.commit("乙", "待清理的正文", None, 0).unwrap();
    // 传 0 天：让它这次就到点
    temp.vault
        .update_settings(None, None, None, Some(0), Some(0), None, None, None, None)
        .unwrap();
    temp.vault.delete("乙").unwrap();

    // 第一次：从没跑过 → 该跑
    let first = temp.vault.run_maintenance().unwrap();
    assert!(first.purged.is_some(), "从没跑过就该跑一次");
    assert!(first.gc.is_some());
    assert_eq!(
        temp.vault.list_trash().unwrap().len(),
        1,
        "保留期下限是 1 天：刚删的那条不该被自动清掉"
    );

    // 再跑一次：刚跑过，间隔没到 → 什么都不做
    let second = temp.vault.run_maintenance().unwrap();
    assert!(second.purged.is_none(), "间隔之内不该重复清");
    assert!(second.gc.is_none());

    // 判定靠"上次执行时间"，所以它必须被记下来
    let view = temp.vault.settings_view();
    assert!(!view.last_trash_purge.is_empty(), "上次清理时间要写回去");
    assert!(!view.last_gc.is_empty(), "上次回收时间要写回去");
}

/// 重定向到**特殊页面**时同样带上来源：虚拟命名空间下的页面也是"页面"，
/// 用户同样需要知道自己是**被带过来的**。
#[test]
fn redirect_to_special_page_carries_the_source() {
    let temp = TempVault::new();
    temp.vault.create("跳到设置").unwrap();
    temp.vault
        .commit(
            "跳到设置",
            "$$COMMAND$$\nREDIRECT: special:settings\n",
            None,
            0,
        )
        .unwrap();

    match temp.vault.parse_address("跳到设置").unwrap() {
        Address::Special { page, via, .. } => {
            assert_eq!(page, "settings");
            let via = via.expect("重定向过来的应当有来源");
            assert_eq!(via.from, "跳到设置");
            assert!(!via.random, "这是重定向，不是随机跳转");
        }
        other => panic!("{other:?}"),
    }
}

/// 命名空间：名称与别名都不许重复，保留名不许占用
#[test]
fn namespace_names_must_be_unique() {
    let mut temp = TempVault::new();
    temp.vault
        .add_namespace("help", vec!["帮助".to_string()], None)
        .unwrap();

    assert!(temp.vault.add_namespace("help", Vec::new(), None).is_err());
    assert!(temp.vault.add_namespace("HELP", Vec::new(), None).is_err());
    assert!(temp
        .vault
        .add_namespace("other", vec![" help ".to_string()], None)
        .is_err());
    assert!(temp
        .vault
        .add_namespace("other", vec!["甲".to_string(), "甲".to_string()], None)
        .is_err());
    assert!(temp.vault.add_namespace("special", Vec::new(), None).is_err());
    assert!(temp.vault.add_namespace("0", Vec::new(), None).is_err());
    assert!(temp.vault.add_namespace("  ", Vec::new(), None).is_err());

    // 别名解析成规范名（链接回显走的就是这条路）
    temp.vault.create("help:入门").unwrap();
    temp.vault.commit("help:入门", "正文", None, 0).unwrap();
    match temp.vault.parse_address("帮助:入门").unwrap() {
        Address::Note { title, .. } => assert_eq!(title, "help:入门"),
        other => panic!("{other:?}"),
    }
}

/// 两跳寻址：目录用**标识**，名字只用于显示与匹配
#[test]
fn namespaces_use_a_stable_id() {
    let mut temp = TempVault::new();
    temp.vault
        .add_namespace("help", vec!["帮助".to_string()], None)
        .unwrap();

    let id = temp
        .vault
        .namespaces()
        .into_iter()
        .find(|item| item.name == "help")
        .expect("新命名空间应当在表里")
        .id;
    assert_ne!(id, "help", "标识是生成的，不该等于名字");

    for title in ["help:甲", "help:乙"] {
        temp.vault.create(title).unwrap();
        temp.vault.commit(title, "正文", None, 0).unwrap();
    }
    assert!(
        temp.root.join("notes").join(&id).is_dir(),
        "笔记应当落在以**标识**命名的目录里"
    );
    assert_eq!(temp.vault.load("help:甲").unwrap().title, "help:甲");

    // 别名认得出同一页
    match temp.vault.parse_address("帮助:甲").unwrap() {
        Address::Note { title, .. } => assert_eq!(title, "help:甲"),
        other => panic!("{other:?}"),
    }

    // 清空：按**名字**调用，内部换成标识；页面进回收站，命名空间还在
    assert_eq!(temp.vault.empty_namespace("help").unwrap(), 2);
    assert_eq!(temp.vault.list_trash().unwrap().len(), 2);

    // 删除（已清空）：条目消失；还原明确报"命名空间不存在"
    assert_eq!(temp.vault.delete_namespace("help").unwrap(), 0);
    let error = temp.vault.restore_note("help:甲").unwrap_err();
    assert!(error.to_string().contains("命名空间"), "{error}");

    // 但那条记录必须还能被清掉（命名空间没了，只能按 id 找），否则永久卡在回收站里
    temp.vault.purge_trash_entry("help:甲").unwrap();
    assert_eq!(temp.vault.list_trash().unwrap().len(), 1);

    // 保留名不可删
    assert!(temp.vault.delete_namespace("special").is_err());
    assert!(temp.vault.delete_namespace("0").is_err());
}

/// 改名只改表里一行：**文件一个都不搬**，链接跟着新名字走
#[test]
fn renaming_a_namespace_moves_nothing() {
    fn dirs(root: &Path) -> Vec<String> {
        let mut out: Vec<String> = fs::read_dir(root.join("notes"))
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .collect();
        out.sort();
        out
    }

    let mut temp = TempVault::new();
    temp.vault.add_namespace("help", Vec::new(), None).unwrap();
    temp.vault.create("help:甲").unwrap();
    temp.vault.commit("help:甲", "正文", None, 0).unwrap();

    let before = dirs(&temp.root);
    temp.vault.rename_namespace("help", "帮助").unwrap();
    assert_eq!(before, dirs(&temp.root), "改名不该动任何目录或文件");

    // 新名字认识它，旧名字不再认识
    assert_eq!(temp.vault.load("帮助:甲").unwrap().title, "帮助:甲");
    assert!(temp.vault.load("help:甲").is_err(), "旧名字已经不属于它了");

    // 旧名字可以给别人用（键是标识，不是名字）
    temp.vault.add_namespace("help", Vec::new(), None).unwrap();
    // 保留名不能改名
    assert!(temp.vault.rename_namespace("special", "别的").is_err());
}

/// 站点地址可以改，也可以清空（清空后变回内容命名空间）
#[test]
fn cross_site_url_is_editable() {
    let mut temp = TempVault::new();

    // 换一个站点地址
    temp.vault
        .update_namespace(
            "zhwiki",
            vec!["中文维基百科".to_string()],
            Some("https://zh.m.wikipedia.org/wiki/$1".to_string()),
        )
        .unwrap();
    let item = temp
        .vault
        .namespaces()
        .into_iter()
        .find(|item| item.name == "zhwiki")
        .unwrap();
    assert_eq!(
        item.site.as_deref(),
        Some("https://zh.m.wikipedia.org/wiki/$1")
    );
    assert!(!item.storable, "有站点地址就是跨站命名空间");

    // 清空站址 → 变回内容命名空间，可以放本仓库的页面
    temp.vault.update_namespace("zhwiki", Vec::new(), None).unwrap();
    let item = temp
        .vault
        .namespaces()
        .into_iter()
        .find(|item| item.name == "zhwiki")
        .unwrap();
    assert!(item.site.is_none());
    assert!(item.storable, "没有站点地址就是本仓库的内容命名空间");
    temp.vault.create("zhwiki:自建条目").unwrap();
    temp.vault
        .commit("zhwiki:自建条目", "正文", None, 0)
        .unwrap();
    match temp.vault.parse_address("zhwiki:自建条目").unwrap() {
        Address::Note { title, .. } => assert_eq!(title, "zhwiki:自建条目"),
        other => panic!("{other:?}"),
    }

    // 保留的命名空间不能配站点地址
    for key in ["special", "0"] {
        assert!(
            temp.vault
                .update_namespace(
                    key,
                    Vec::new(),
                    Some("https://example.com/$1".to_string()),
                )
                .is_err(),
            "{key} 不该能配站点地址"
        );
    }
}

/// 模板命名空间里的页面可以当模板用：`::名字` 就是那一页
#[test]
fn template_page_is_used_by_name() {
    let temp = TempVault::new();

    // markdown 模板：`{{参数}}` 取参数，`{{body}}` 取块内容
    temp.vault.create("template:盒子").unwrap();
    temp.vault
        .commit("template:盒子", "**{{origin}}**\n\n{{body}}\n", None, 0)
        .unwrap();

    temp.vault.create("用了模板的页").unwrap();
    temp.vault
        .commit("用了模板的页", "::盒子 origin=标题\n  内容一行\n", None, 0)
        .unwrap();

    let html = temp
        .vault
        .load_outcome("用了模板的页")
        .unwrap()
        .note
        .unwrap()
        .html;
    assert!(html.contains("<strong>标题</strong>"), "参数应当填进去：{html}");
    assert!(html.contains("内容一行"), "块内容应当填进去：{html}");

    // .css 模板页：用 `::css src=页面名` 取；注入的样式能直接读我们的变量
    temp.vault.create("template:样式.css").unwrap();
    temp.vault
        .commit("template:样式.css", ".x { color: var(--accent); }\n", None, 0)
        .unwrap();
    temp.vault.create("带样式的页").unwrap();
    temp.vault
        .commit("带样式的页", "::css src=样式.css\n", None, 0)
        .unwrap();

    let html = temp
        .vault
        .load_outcome("带样式的页")
        .unwrap()
        .note
        .unwrap()
        .html;
    assert!(html.contains("<style>"), "{html}");
    assert!(html.contains("var(--accent)"), "{html}");
}

/// 模板命名空间里的 .css / .html 页面：阅读时当代码块显示，不按 markdown 解析
#[test]
fn css_template_renders_as_code_block() {
    let temp = TempVault::new();
    temp.vault.create("template:样式.css").unwrap();
    temp.vault
        .commit(
            "template:样式.css",
            ".note-body { color: red; }\n\n# 这不是标题\n",
            None,
            0,
        )
        .unwrap();

    let note = temp
        .vault
        .load_outcome("template:样式.css")
        .unwrap()
        .note
        .expect("应当读得到");
    assert!(
        note.html.contains("language-css"),
        "应当当代码块渲染：{}",
        note.html
    );
    // 前端据此选 CM6 语言：判定与渲染共用同一个结果
    assert_eq!(note.language.as_deref(), Some("css"));
    assert!(
        !note.html.contains("<h1"),
        "不该按 markdown 解析：{}",
        note.html
    );
    // 原始文本照旧给编辑器
    assert!(note.markdown.contains(".note-body"));

    // 主命名空间里叫同样名字的笔记不受影响
    temp.vault.create("样式.css").unwrap();
    temp.vault.commit("样式.css", "# 这是标题\n", None, 0).unwrap();
    let main = temp.vault.load_outcome("样式.css").unwrap().note.unwrap();
    assert!(
        main.html.contains("<h1"),
        "主命名空间里的同名笔记照常解析：{}",
        main.html
    );
    assert_eq!(main.language, None, "别处叫 .css 的普通笔记仍按 markdown");
}

/// 模板命名空间：内置、可存储、保留（删不掉也改不了名），但照常放页面、走完整链路
#[test]
fn template_namespace_is_builtin() {
    let mut temp = TempVault::new();
    let template = temp
        .vault
        .namespaces()
        .into_iter()
        .find(|item| item.id == "template")
        .expect("应当默认有 template 命名空间");
    assert_eq!(template.name, "template");
    assert!(template.storable, "里面放的就是普通笔记");
    assert!(template.site.is_none());

    // 保留名：与主命名空间、special 一样，删不掉也改不了名
    assert!(temp.vault.delete_namespace("template").is_err());
    assert!(temp.vault.rename_namespace("template", "别的").is_err());

    // 但它照常能放页面，历史与草稿也照常
    temp.vault.create("template:我的模板").unwrap();
    temp.vault
        .commit("template:我的模板", "正文", None, 0)
        .unwrap();
    temp.vault.save_draft("template:我的模板", "草稿", 1).unwrap();
    assert!(temp.vault.load_draft("template:我的模板").unwrap().is_some());
    match temp.vault.parse_address("template:我的模板").unwrap() {
        Address::Note { title, .. } => assert_eq!(title, "template:我的模板"),
        other => panic!("{other:?}"),
    }
}

/// 默认就有两个跨站命名空间：`zhwiki` 与 `qw`
#[test]
fn default_cross_site_namespaces() {
    let temp = TempVault::new();
    let table = temp.vault.namespaces();

    let zhwiki = table
        .iter()
        .find(|item| item.name == "zhwiki")
        .expect("应当默认有 zhwiki");
    assert!(!zhwiki.storable, "跨站命名空间：页面在别的站上");
    assert_eq!(
        zhwiki.site.as_deref(),
        Some("https://zh.wikipedia.org/wiki/$1")
    );
    assert_eq!(zhwiki.aliases, vec!["中文维基百科".to_string()]);

    let qw = table
        .iter()
        .find(|item| item.name == "qw")
        .expect("应当默认有 qw");
    assert_eq!(
        qw.site.as_deref(),
        Some("https://www.qiuwenbaike.cn/wiki/$1")
    );
    assert_eq!(qw.aliases, vec!["求闻百科".to_string()]);
}

/// 删掉默认的跨站命名空间之后**不会自己回来** —— "默认就有"不等于"删不掉"
#[test]
fn deleting_a_default_namespace_sticks() {
    let mut temp = TempVault::new();
    temp.vault.delete_namespace("zhwiki").unwrap();
    assert!(
        !temp
            .vault
            .namespaces()
            .iter()
            .any(|item| item.name == "zhwiki"),
        "删掉之后就没了"
    );

    // 从磁盘重新读一次（模拟重开应用）：不该把它补回来
    let text = fs::read_to_string(temp.root.join("namespaces.json")).unwrap();
    let mut table: NamespaceTable = serde_json::from_str(&text).unwrap();
    assert!(!table.sow_defaults(), "已经播过种，不该再补");
    assert!(
        !table.items.iter().any(|item| item.name == "zhwiki"),
        "重开之后也不该回来"
    );
    assert!(
        table.items.iter().any(|item| item.name == "qw"),
        "没删的那条还在"
    );
}

/// 跨站链接：前缀配了站点地址 → 渲染成带 href 的绿链，不在本仓库里查页面
#[test]
fn interwiki_links_point_at_another_site() {
    let temp = TempVault::new();
    // `zhwiki` 是默认就有的跨站命名空间（见下面的 default_cross_site_namespaces），不必自己加
    let item = temp
        .vault
        .namespaces()
        .into_iter()
        .find(|item| item.name == "zhwiki")
        .unwrap();
    assert!(!item.storable, "配了站点的命名空间页面在别处，本仓库不存");
    assert_eq!(
        item.url_for("New York").unwrap(),
        "https://zh.wikipedia.org/wiki/New_York",
        "空格按 MediaWiki 习惯折成下划线"
    );

    temp.vault.create("引用").unwrap();
    temp.vault
        .commit("引用", "看 [[zhwiki:NASA|NASA]]。", None, 0)
        .unwrap();

    let html = temp.vault.load("引用").unwrap().html;
    assert!(
        html.contains(r#"href="https://zh.wikipedia.org/wiki/NASA""#),
        "{html}"
    );
    assert!(html.contains(r#"data-interwiki="true""#), "{html}");
    assert!(
        !html.contains("data-missing"),
        "跨站链接不该被判成红链（那是本仓库有没有这一页的事）：{html}"
    );
}

/// 别名可增可减；保留的两个也能配别名（special 的别名要真的路由过去）；
/// 主命名空间可以清空
#[test]
fn aliases_are_editable_everywhere() {
    let mut temp = TempVault::new();
    temp.vault.add_namespace("help", Vec::new(), None).unwrap();
    temp.vault.create("help:条目").unwrap();
    temp.vault.commit("help:条目", "正文", None, 0).unwrap();

    // 一次给两个别名，两个都认
    temp.vault
        .update_namespace("help", vec!["帮助".to_string(), "百科".to_string()], None)
        .unwrap();
    for alias in ["帮助", "百科"] {
        let address = format!("{alias}:条目");
        match temp.vault.parse_address(&address).unwrap() {
            Address::Note { title, .. } => assert_eq!(title, "help:条目"),
            other => panic!("{address} → {other:?}"),
        }
    }

    // 删到一个：去掉的那个不再认
    temp.vault
        .update_namespace("help", vec!["帮助".to_string()], None)
        .unwrap();
    assert!(temp.vault.parse_address("百科:条目").is_err(), "别名已删掉");

    // 别名之间不许重复（大小写与空白不影响判重）
    assert!(temp
        .vault
        .update_namespace("help", vec!["帮助".to_string(), " 帮助 ".to_string()], None)
        .is_err());

    // special 的别名要真的路由到特殊页面
    temp.vault
        .update_namespace("special", vec!["特殊".to_string()], None)
        .unwrap();
    match temp.vault.parse_address("特殊:gc").unwrap() {
        Address::Special { page, .. } => assert_eq!(page, "gc"),
        other => panic!("{other:?}"),
    }

    // 主命名空间也能配别名：`主:某页` 落在主命名空间。
    // 没建过那一页时应当是"缺失"，而不是报标题非法 —— 这说明前缀被正确认成了
    // 主命名空间（主命名空间没有前缀，所以显示标题里看不到它）。
    temp.vault
        .update_namespace("0", vec!["主".to_string()], None)
        .unwrap();
    match temp.vault.parse_address("主:不存在的页").unwrap() {
        Address::Missing { title, .. } => assert_eq!(title, "不存在的页"),
        other => panic!("{other:?}"),
    }
    temp.vault.create("主:另一页").unwrap();
    temp.vault.commit("主:另一页", "正文", None, 0).unwrap();
    match temp.vault.parse_address("主:另一页").unwrap() {
        Address::Note { title, .. } => assert_eq!(title, "另一页"),
        other => panic!("{other:?}"),
    }

    // 主命名空间可以**清空**（它只不能改名与删除）
    temp.vault.create("常规条目").unwrap();
    temp.vault.commit("常规条目", "正文", None, 0).unwrap();
    assert!(temp.vault.empty_namespace("0").unwrap() >= 1);
    assert!(
        !temp
            .vault
            .list_notes()
            .unwrap()
            .iter()
            .any(|item| item.title == "常规条目"),
        "清空之后主命名空间里不该还有它"
    );
    assert!(temp.vault.delete_namespace("0").is_err());
}

#[test]
fn default_root_is_under_home() {
    let root = default_root().expect("应当能定位到主目录");
    assert_eq!(
        root.file_name().and_then(|name| name.to_str()),
        Some(DEFAULT_DIR_NAME)
    );
    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        assert!(root.starts_with(&home), "{} 应当在家目录下", root.display());
    }
}

#[test]
fn history_lists_commits_and_drafts() {
    let temp = TempVault::new();
    temp.vault.create("历史").unwrap();
    temp.vault.commit("历史", "第一版", Some("初稿"), 0).unwrap();
    temp.vault.save_draft("历史", "第一版又加了一点", 1).unwrap();
    temp.vault.commit("历史", "第二版", Some("定稿"), 1).unwrap();

    let history = temp.vault.history("历史").unwrap();
    let kinds: Vec<&str> = history.iter().map(|item| item.kind.as_str()).collect();
    assert_eq!(kinds, vec!["create", "commit", "draft", "commit"]);
    assert_eq!(history[1].summary.as_deref(), Some("初稿"));
    assert_eq!(history[3].rev, 3);
    assert_eq!(history[3].supersedes, vec![2]);
    assert_eq!(history[1].bytes, 9, "「第一版」是 3 个 CJK 字");
    assert_eq!(history[1].delta, 9);
}

#[test]
fn revision_reads_any_version_including_drafts() {
    let temp = TempVault::new();
    temp.vault.create("版本").unwrap();
    temp.vault.commit("版本", "第一版", None, 0).unwrap();
    temp.vault.save_draft("版本", "草稿内容", 1).unwrap();

    let first = temp.vault.revision("版本", 1).unwrap();
    assert_eq!(first.kind, "commit");
    assert_eq!(first.markdown, "第一版");
    assert!(first.html.contains("第一版"), "{}", first.html);

    let draft = temp.vault.revision("版本", 2).unwrap();
    assert_eq!(draft.kind, "draft");
    assert_eq!(draft.markdown, "草稿内容");

    assert!(matches!(
        temp.vault.revision("版本", 99),
        Err(VaultError::RevisionNotFound { rev: 99, .. })
    ));
}

#[test]
fn compare_reports_changes_between_versions() {
    let temp = TempVault::new();
    temp.vault.create("对比").unwrap();
    temp.vault.commit("对比", "一\n二\n三", None, 0).unwrap();
    temp.vault.commit("对比", "一\n改\n三", None, 1).unwrap();

    let result = temp.vault.compare("对比", 1, 2).unwrap();
    assert_eq!(result.from_rev, 1);
    assert_eq!(result.to_rev, 2);
    assert_eq!(result.inserted, 1);
    assert_eq!(result.deleted, 1);
    assert!(result
        .lines
        .iter()
        .any(|line| line.kind == "insert" && line.text == "改"));
}

/// 小改动应当真的用上增量，而且回放必须逐字节还原
#[test]
fn small_edits_are_stored_as_deltas() {
    let temp = TempVault::new();
    temp.vault.create("增量").unwrap();
    let first = "一\n二\n三\n四\n五\n六\n七\n八\n九\n十\n";
    temp.vault.commit("增量", first, None, 0).unwrap();

    let second = first.replace("五", "改过的五");
    let note = temp.vault.commit("增量", &second, None, 1).unwrap();
    assert_eq!(note.markdown, second, "增量回放必须逐字节还原");

    let events = temp.vault.events_for("增量").unwrap();
    let encodings: Vec<String> = events
        .iter()
        .filter_map(|event| match event {
            Event::Rev { encoding, .. } => Some(encoding.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(encodings, vec!["full", "delta"], "第二版应当用增量");

    let history = temp.vault.history("增量").unwrap();
    assert_eq!(history[2].encoding, "delta");
    assert_eq!(
        history[2].bytes as usize,
        second.len(),
        "bytes 记的是完整内容的大小，不是补丁大小"
    );
}

/// 大改动时补丁反而更大，必须退回整份快照（规则一）
#[test]
fn heavy_rewrites_fall_back_to_full_snapshots() {
    let temp = TempVault::new();
    temp.vault.create("重写").unwrap();
    temp.vault.commit("重写", "甲\n", None, 0).unwrap();

    let mut target = String::new();
    for index in 0..200 {
        target.push_str(&format!("全新的第 {index} 行内容\n"));
    }
    let note = temp.vault.commit("重写", &target, None, 1).unwrap();
    assert_eq!(note.markdown, target);

    let encoding = temp
        .vault
        .events_for("重写")
        .unwrap()
        .iter()
        .find_map(|event| match event {
            Event::Rev { rev: 2, encoding, .. } => Some(encoding.clone()),
            _ => None,
        })
        .unwrap();
    assert_eq!(encoding, "full", "大改动应当存整份");
}

/// 增量链到上限后必须再落一次整份快照，且每版都还要能还原（规则二）
#[test]
fn chain_length_is_capped() {
    let temp = TempVault::new();
    temp.vault.create("链长").unwrap();

    let mut text = String::from("第一版\n");
    temp.vault.commit("链长", &text, None, 0).unwrap();
    for round in 0..34 {
        text.push_str(&format!("第 {round} 次追加\n"));
        temp.vault
            .commit("链长", &text, None, (round + 1) as u64)
            .unwrap();
    }

    let encodings: Vec<String> = temp
        .vault
        .events_for("链长")
        .unwrap()
        .iter()
        .filter_map(|event| match event {
            Event::Rev { encoding, .. } => Some(encoding.clone()),
            _ => None,
        })
        .collect();
    let fulls = encodings.iter().filter(|item| item.as_str() == "full").count();
    assert!(fulls >= 2, "链到上限后应当再落一次整份快照：{encodings:?}");

    // 最新一版与中间某一版都要能正确回放
    assert_eq!(temp.vault.load("链长").unwrap().markdown, text);
    let mut fifth = String::from("第一版\n");
    for round in 0..4 {
        fifth.push_str(&format!("第 {round} 次追加\n"));
    }
    assert_eq!(temp.vault.revision("链长", 5).unwrap().markdown, fifth);
}

/// 提交的基准只能是提交，绝不能是草稿（规则三）
#[test]
fn commits_never_depend_on_drafts() {
    let temp = TempVault::new();
    temp.vault.create("依赖").unwrap();
    temp.vault.commit("依赖", "v1", None, 0).unwrap();
    temp.vault.save_draft("依赖", "草稿", 1).unwrap();
    temp.vault.commit("依赖", "v2", None, 1).unwrap();

    let bases: Vec<Option<u64>> = temp
        .vault
        .events_for("依赖")
        .unwrap()
        .iter()
        .filter_map(|event| match event {
            Event::Rev { base_rev, .. } => Some(*base_rev),
            _ => None,
        })
        .collect();
    assert!(
        bases.iter().all(|base| *base != Some(2)),
        "提交的基准不能指向草稿（版本 2）：{bases:?}"
    );

    // 草稿被清理之后内容仍然读得出来 —— 这正是这条规则要保证的
    temp.vault.prune("依赖").unwrap();
    assert_eq!(temp.vault.load("依赖").unwrap().markdown, "v2");
}

#[test]
fn a_renamed_version_keeps_the_title_of_its_time() {
    let temp = TempVault::new();
    temp.vault.create("旧标题").unwrap();
    temp.vault.commit("旧标题", "正文", None, 0).unwrap();
    temp.vault.rename("旧标题", "新标题").unwrap();

    let old = temp.vault.revision("新标题", 1).unwrap();
    assert_eq!(old.title, "旧标题", "版本 1 当时的标题还是旧的");
    let new = temp.vault.revision("新标题", 2).unwrap();
    assert_eq!(new.title, "新标题");
    assert_eq!(new.markdown, "正文");
}

/// 草稿事件数（自动保存只该在**真的变了**的时候追加）
fn draft_event_count(vault: &Vault, title: &str) -> usize {
    vault
        .events_for(title)
        .unwrap()
        .iter()
        .filter(|event| matches!(event, Event::Auto { .. }))
        .count()
}

/// 草稿没变动就不再存草稿：自动保存反复调用，空版本会把历史灌满
#[test]
fn unchanged_draft_is_not_saved_again() {
    let temp = TempVault::new();
    temp.vault.create("草稿页").unwrap();
    temp.vault.commit("草稿页", "第一版", None, 0).unwrap();

    // 与**正文**一致：不该产生草稿。
    // 提交之后自动保存常常就是这样调的 —— 草稿已被提交取代，再存就等于凭空多一个空版本。
    temp.vault.save_draft("草稿页", "第一版", 1).unwrap();
    assert!(
        temp.vault.load_draft("草稿页").unwrap().is_none(),
        "与正文一致时不该有草稿"
    );
    assert_eq!(draft_event_count(&temp.vault, "草稿页"), 0);

    // 真的改了：存一份
    temp.vault.save_draft("草稿页", "第二版", 1).unwrap();
    assert!(temp.vault.load_draft("草稿页").unwrap().is_some());
    assert_eq!(draft_event_count(&temp.vault, "草稿页"), 1);

    // 同样的内容再存：不该追加
    temp.vault.save_draft("草稿页", "第二版", 1).unwrap();
    assert_eq!(
        draft_event_count(&temp.vault, "草稿页"),
        1,
        "同样的草稿不该重复追加"
    );

    // 提交草稿 → 再存同样的内容：仍然不该冒出一个新草稿。
    // 提交后 rev 是 3：草稿自己占了一版（草稿也是链上的一版）。
    temp.vault.commit("草稿页", "第二版", None, 1).unwrap();
    temp.vault.save_draft("草稿页", "第二版", 3).unwrap();
    assert!(
        temp.vault.load_draft("草稿页").unwrap().is_none(),
        "刚提交过的内容不该又变成草稿"
    );
    assert_eq!(draft_event_count(&temp.vault, "草稿页"), 1, "自动保存不该再追加");
}

/// 诊断报告：分段齐全，且能说清每条模板块的走向
#[test]
fn debug_report_covers_the_basics() {
    let temp = TempVault::new();
    temp.vault.create("有模板的一页").unwrap();
    temp.vault
        .commit(
            "有模板的一页",
            "::quote\n  内容\n::没有这个\n  内容\n",
            None,
            0,
        )
        .unwrap();

    let report = temp.vault.debug_report(Some("有模板的一页"));
    let titles: Vec<&str> = report.sections.iter().map(|s| s.title.as_str()).collect();
    for expected in ["版本与环境", "设置", "命名空间", "内容与体积", "当前页"] {
        assert!(titles.contains(&expected), "缺少 {expected}：{titles:?}");
    }

    // 探针要说清每条模板块的走向：内建的、查不到的
    let current = report
        .sections
        .iter()
        .find(|section| section.title == "当前页")
        .unwrap();
    let routes: Vec<&str> = current
        .entries
        .iter()
        .filter(|entry| entry.label.starts_with("::"))
        .map(|entry| entry.value.as_str())
        .collect();
    assert!(routes.iter().any(|route| *route == "内建模板"), "{routes:?}");
    assert!(
        routes.iter().any(|route| route.contains("未知模板")),
        "{routes:?}"
    );

    // 模板页清单也要出现
    let storage = report
        .sections
        .iter()
        .find(|section| section.title == "内容与体积")
        .unwrap();
    assert!(
        storage
            .entries
            .iter()
            .any(|entry| entry.label.contains("模板页")),
        "应当列出模板页"
    );
}
