//! 库布局的测试。
//!
//! 重点是**第一次打开就该是完整的** —— 目录与那些 JSON 在打开时按需建出来，
//! 不必等写了一次才出现。

use super::*;

use super::*;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "refind-note-database-test-{}-{name}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    dir
}

/// 从暂存目录开一个库（工作目录的骨架先建出来）
fn open(dir: &Path) -> Result<Database, String> {
    let workspace = Workspace::open(dir.to_path_buf())?;
    Database::open(&workspace)
}

#[test]
fn a_fresh_directory_becomes_a_database() {
    let root = scratch("fresh");
    let database = open(&root).unwrap();

    assert_eq!(database.meta().kind, KIND);
    assert_eq!(database.meta().version, MODEL_VERSION);
    OffsetDateTime::parse(&database.meta().created_at, &Rfc3339).expect("该是 RFC3339");

    // 目录骨架与那几张表第一次打开就该在
    assert!(database
        .blobs()
        .path_of("x")
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .is_dir());
    assert!(database.drafts_dir().is_dir());
    // 空表**不写**：写下来会被同步当成"本机改过"（新机器上尤其），
    // 读到的是默认值，第一次真正改名/建页时才落盘
    assert!(!database.titles_path().is_file());
    assert!(
        database.namespaces().get("0").is_some(),
        "内建命名空间要读得到"
    );
    // 压缩默认开着：正文多半是文本，压一下几乎总是划算，而且无感
    let default_protection = database.config().protection;
    assert!(default_protection.compress);
    assert!(default_protection.gpg_sign.is_none());
    assert!(default_protection.gpg_encrypt.is_none());
    assert!(!default_protection.symmetric);

    // 重新打开认得出来，且不新建
    let again = open(&root).unwrap();
    assert_eq!(again.meta().created_at, database.meta().created_at);

    let _ = fs::remove_dir_all(&root);
}

/// 仓库规模数得对（诊断页报的就是这一串）
///
/// 顺带钉住路径的基准：`root` **就是 `db/` 那一层** —— 数内容块时写成
/// `db/blobs` 会数到零（第一版就写错过）。
#[test]
fn facts_count_what_is_really_there() {
    let root = scratch("facts");
    let database = open(&root).unwrap();

    // 刚建出来的库：三处都空着（`db/` 目录本身已经在了）
    let empty = database.facts();
    assert_eq!(empty.logs, 0, "{empty:?}");
    assert_eq!(empty.blobs, 0, "{empty:?}");
    assert_eq!(empty.drafts, 0, "{empty:?}");
    assert_eq!(empty.trash, 0, "{empty:?}");

    // 各丢一份进去：内容块（内容寻址那两级目录）、事件日志、草稿、回收站
    let blob = database.blobs().path_of("abc123");
    fs::create_dir_all(blob.parent().unwrap()).unwrap();
    fs::write(&blob, vec![0u8; 100]).unwrap();

    let log = database.objects_dir().join(MAIN_ID).join("1.log");
    fs::write(&log, "{}\n").unwrap();
    let draft = database.drafts_dir().join("1");
    fs::write(&draft, "写了一半").unwrap();
    let trashed = database.trash_dir().join(MAIN_ID).join("2.log");
    fs::create_dir_all(trashed.parent().unwrap()).unwrap();
    fs::write(&trashed, "{}\n").unwrap();

    let facts = database.facts();
    assert_eq!(facts.logs, 1, "{facts:?}");
    assert_eq!(facts.blobs, 1, "{facts:?}");
    assert_eq!(facts.blob_bytes, 100, "{facts:?}");
    assert_eq!(facts.drafts, 1, "{facts:?}");
    assert_eq!(facts.trash, 1, "{facts:?}");
    assert!(facts.database_bytes >= 100, "{facts:?}");

    let _ = fs::remove_dir_all(&root);
}

/// 老仓库里的 `config.json` 会被搬成 `repository.json`，设置不丢
#[test]
fn the_old_config_file_is_moved_to_its_new_name() {
    let root = scratch("legacy-config");
    {
        // 老样子：先用旧名字写一份非默认的设置
        let workspace = Workspace::open(root.clone()).unwrap();
        let settings = workspace.settings_dir();
        fs::create_dir_all(&settings).unwrap();
        fs::write(
            settings.join(LEGACY_CONFIG_FILE),
            r#"{"trash_keep_days":7,"gc_interval_days":3}"#,
        )
        .unwrap();
    }

    let database = open(&root).unwrap();
    assert!(
        !database.settings.join(LEGACY_CONFIG_FILE).exists(),
        "旧的该搬走"
    );
    assert!(database.settings.join(CONFIG_FILE).is_file());

    // 搬过来的是**内容**，不是一份新的默认值
    let config = database.config();
    assert_eq!(config.trash_keep_days, 7);
    assert_eq!(config.gc_interval_days, 3);

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_foreign_directory_is_refused() {
    let root = scratch("foreign");
    let database_dir = root.join("db");
    fs::create_dir_all(&database_dir).unwrap();
    fs::write(
        database_dir.join(META_FILE),
        r#"{"kind":"别的东西","version":"1.0.0","created_at":"2026-01-01T00:00:00Z"}"#,
    )
    .unwrap();

    let error = open(&root).unwrap_err();
    assert!(error.contains("不是重逢笔记的数据库"), "{error}");

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_version_mismatch_is_refused() {
    let root = scratch("version");
    let database_dir = root.join("db");
    fs::create_dir_all(&database_dir).unwrap();
    fs::write(
        database_dir.join(META_FILE),
        r#"{"kind":"refind-note","version":"9.9.9","created_at":"2026-01-01T00:00:00Z"}"#,
    )
    .unwrap();

    let error = open(&root).unwrap_err();
    assert!(
        error.contains("9.9.9") && error.contains(MODEL_VERSION),
        "{error}"
    );

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn broken_metadata_is_refused_instead_of_being_overwritten() {
    let root = scratch("broken");
    let database_dir = root.join("db");
    fs::create_dir_all(&database_dir).unwrap();
    fs::write(database_dir.join(META_FILE), "{ 这不是 JSON").unwrap();

    assert!(open(&root).is_err());
    // 没有被当成"新建"而覆盖掉 —— 内容还在，人还能去看
    assert!(fs::read_to_string(database_dir.join(META_FILE))
        .unwrap()
        .contains("这不是 JSON"));

    let _ = fs::remove_dir_all(&root);
}
