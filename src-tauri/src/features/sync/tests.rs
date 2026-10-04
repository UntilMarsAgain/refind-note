//! 纯逻辑的单元测试：判定规则（`decide`）与云端那一层（封/解），都不碰网络。
//! 真跑一遍整条流程的见 `end_to_end`。

use super::*;
use crate::features::sync::cipher::{CloudCipher, CloudEnvelope};
use crate::features::sync::rules::{decide, CloudSays, Decision, Evidence, Local, Remote};
use crate::features::sync::settings::{Stamp, SyncSettings, SyncSettingsPatch};
use crate::storage::s3::S3Config;

fn local(hash: &str, mtime: i64) -> Local {
    Local {
        hash: hash.to_string(),
        mtime,
    }
}

fn remote(etag: &str, modified: i64) -> Remote {
    Remote {
        size: 10,
        etag: etag.to_string(),
        modified,
    }
}

fn stamp(hash: &str, mtime: i64, etag: &str) -> Stamp {
    Stamp {
        hash: hash.to_string(),
        mtime,
        etag: etag.to_string(),
    }
}

/// **交给界面的那一份里不许有秘密**：密钥与 S3 私钥都只报"有没有"。
///
/// 这条是给后来者看的护栏：哪天有人图省事把 `key` 加回 view，测试当场就红。
#[test]
fn the_view_carries_no_secrets() {
    let settings = SyncSettings {
        enabled: true,
        encrypt: true,
        cipher: crate::storage::codec::Cipher::Aes256Gcm,
        reupload: false,
        key: "SUPER-SECRET-CLOUD-KEY".to_string(),
        s3: S3Config {
            endpoint: "https://s3.example.com".to_string(),
            bucket: "b".to_string(),
            access_key: "AK".to_string(),
            secret_key: "SUPER-SECRET-S3".to_string(),
            ..Default::default()
        },
    };

    let shown = serde_json::to_string(&settings.view()).unwrap();
    assert!(!shown.contains("SUPER-SECRET"), "{shown}");
    assert!(shown.contains("\"has_key\":true"), "{shown}");
    assert!(shown.contains("\"has_secret\":true"), "{shown}");

    // 界面交回来时私钥留空 = **不改**（它本来就拿不到原来那一把）
    let mut target = settings.clone();
    target.apply(SyncSettingsPatch {
        enabled: false,
        encrypt: true,
        cipher: crate::storage::codec::Cipher::Aes256Gcm,
        endpoint: "https://other".to_string(),
        ..Default::default()
    });
    assert_eq!(target.s3.secret_key, "SUPER-SECRET-S3");
    assert_eq!(target.s3.endpoint, "https://other");
    assert_eq!(target.key, "SUPER-SECRET-CLOUD-KEY");
}

/// 云端那一层：封了能解、封出来的每一次都不一样、换了钥匙解不开
#[test]
fn the_cloud_layer_round_trips() {
    let key = generate_key().unwrap();
    let cipher = CloudCipher::from_key(&key).unwrap();

    let sealed = cipher.seal("db/titles.json", b"hello".to_vec()).unwrap();
    assert_ne!(sealed, b"hello", "传上去的不该是明文");
    assert!(CloudEnvelope::matches(&sealed), "自带说明：这是本程序封的");
    assert_eq!(
        cipher.open("db/titles.json", sealed.clone()).unwrap(),
        b"hello"
    );

    // 同一份内容封两次：nonce 不同，密文就不同
    let again = cipher.seal("db/titles.json", b"hello".to_vec()).unwrap();
    assert_ne!(sealed, again);

    // 换一把钥匙：解不开（而且说得清是钥匙的问题）
    let other = CloudCipher::from_key(&generate_key().unwrap()).unwrap();
    let error = other.open("db/titles.json", sealed).unwrap_err();
    assert!(error.contains("密钥"), "{error}");
}

/// 没封过的（开加密之前传上去的那些）：原样放行 ——
/// 换了设置不该把已经躺在云端的东西读废
#[test]
fn a_plain_object_still_comes_back() {
    let cipher = CloudCipher::from_key(&generate_key().unwrap()).unwrap();
    let plain = br#"{"kind":"x"}"#.to_vec();
    assert_eq!(
        cipher.open("db/meta.json", plain.clone()).unwrap(),
        plain,
        "不是我们封的，就照原样用"
    );
}

/// 抄错的钥匙当场说清楚（少几个字、多几个字都要认得出）
#[test]
fn a_mistyped_key_is_refused_early() {
    assert!(CloudCipher::from_key("这不是 base64").is_err());
    assert!(CloudCipher::from_key("AAAA").is_err(), "长度不对也要拦下");
    assert!(CloudCipher::from_key(&generate_key().unwrap()).is_ok());
}

#[test]
fn only_what_belongs_to_the_repository_is_synced() {
    assert!(is_synced("db/blobs/ab/abc"));
    assert!(is_synced("db/objects/0/1.log"));
    assert!(is_synced("db/titles.json"));
    assert!(is_synced("settings/repository.json"));

    // 这台机器自己的东西：不传
    assert!(!is_synced("settings/preferences.json"));
    assert!(!is_synced("settings/browsing.jsonl"));
    assert!(!is_synced("settings/sync.json"));
    assert!(!is_synced("settings/sync-index.json"));
    assert!(!is_synced("db/drafts/abc"));
}

/// 没动过的两边（索引也对得上）：什么也不做
#[test]
fn a_quiet_pair_does_nothing() {
    let (decision, _) = decide(
        false,
        Some(&local("h1", 100)),
        Some(&remote("aaa", 100)),
        Some(&stamp("h1", 100, "aaa")),
        Evidence::none(),
    );
    assert_eq!(decision, Decision::Nothing);
}

#[test]
fn only_the_local_side_changed_uploads() {
    let (decision, _) = decide(
        false,
        Some(&local("h2", 200)),
        Some(&remote("aaa", 100)),
        Some(&stamp("h1", 100, "aaa")),
        Evidence::none(),
    );
    assert_eq!(decision, Decision::Upload);
}

#[test]
fn only_the_remote_side_changed_downloads() {
    let (decision, _) = decide(
        false,
        Some(&local("h1", 100)),
        Some(&remote("bbb", 200)),
        Some(&stamp("h1", 100, "aaa")),
        Evidence::none(),
    );
    assert_eq!(decision, Decision::Download);
}

/// 两边都动过：云端更新就取云端，本机不旧就留本机（**不静默丢东西**，会记一条）
#[test]
fn a_real_conflict_is_settled_by_time() {
    let (decision, why) = decide(
        false,
        Some(&local("h2", 200)),
        Some(&remote("bbb", 300)),
        Some(&stamp("h1", 100, "aaa")),
        Evidence::none(),
    );
    assert_eq!(decision, Decision::TakeNewer("remote"));
    assert!(!why.is_empty(), "取哪边、为什么，要说得出来");

    let (decision, _) = decide(
        false,
        Some(&local("h2", 400)),
        Some(&remote("bbb", 300)),
        Some(&stamp("h1", 100, "aaa")),
        Evidence::none(),
    );
    assert_eq!(decision, Decision::TakeNewer("local"));

    // 时间几乎一样时保守：留本机那份（宁可多传一次，也别把刚写的盖掉）
    let (decision, _) = decide(
        false,
        Some(&local("h2", 300)),
        Some(&remote("bbb", 300)),
        Some(&stamp("h1", 100, "aaa")),
        Evidence::none(),
    );
    assert_eq!(decision, Decision::TakeNewer("local"));
}

/// **清单空白绝不能当成"云端删过"** —— 这是真出过的事故：
///
/// 桶名被填成了带桶的端点（`https://桶.s3.某云.net`），客户端又按"桶名走路径"拼 URL，
/// 于是上传把桶名写进了键、列举永远列不到东西。结果每同步一次，就把本机那些
/// "账上记着已对齐"的文件当成"云端删过的"删掉一批 —— 用户刚写的页面就是这么没的。
#[test]
fn a_missing_listing_never_means_the_cloud_deleted_it() {
    let here = local("h1", 100);
    let stamp = stamp("h1", 100, "aaa");

    // 云端账上还记着它（只是清单里没列出来）→ 传上去，绝不删
    let (decision, why) = decide(
        false,
        Some(&here),
        None,
        Some(&stamp),
        Evidence {
            cloud: CloudSays::Listed,
            trashed: false,
        },
    );
    assert_eq!(decision, Decision::Upload, "{why}");

    // 云端连账都没有（第一次用、换了桶）→ 同样没有证据，同样不删
    let (decision, _) = decide(false, Some(&here), None, Some(&stamp), Evidence::none());
    assert_eq!(decision, Decision::Upload);
}

/// 云端有、本机没有，但**本机这边找不到"是删掉的"凭据** → 取回来，别替人抹掉
#[test]
fn a_cloud_file_is_restored_unless_the_deletion_left_a_trace() {
    let there = remote("aaa", 100);

    // 回收站里没有它：取回本机
    let (decision, why) = decide(
        false,
        None,
        Some(&there),
        Some(&stamp("h1", 100, "aaa")),
        Evidence {
            cloud: CloudSays::Gone,
            trashed: false,
        },
    );
    assert_eq!(decision, Decision::Download, "{why}");

    // 回收站里有它：本机确实删过，云端跟着删
    let (decision, _) = decide(
        false,
        None,
        Some(&there),
        Some(&stamp("h1", 100, "aaa")),
        Evidence {
            cloud: CloudSays::Gone,
            trashed: true,
        },
    );
    assert_eq!(decision, Decision::DeleteRemote);
}

/// 新写的传上去，云端多出来的拿回来
#[test]
fn a_first_sync_takes_the_union() {
    let (decision, _) = decide(false, Some(&local("h1", 100)), None, None, Evidence::none());
    assert_eq!(decision, Decision::Upload);

    let (decision, _) = decide(
        false,
        None,
        Some(&remote("aaa", 100)),
        None,
        Evidence::none(),
    );
    assert_eq!(decision, Decision::Download);
}

/// 删除要**两边都认**、**而且有凭据**才动
#[test]
fn deletions_travel_only_when_the_other_side_is_untouched() {
    // 本机删了（回收站里留着凭据）、云端还在 → 云端也删
    let (decision, why) = decide(
        false,
        None,
        Some(&remote("aaa", 100)),
        Some(&stamp("h1", 100, "aaa")),
        Evidence {
            cloud: CloudSays::Gone,
            trashed: true,
        },
    );
    assert_eq!(decision, Decision::DeleteRemote);
    assert!(!why.is_empty());

    // 云端账上也说没了，本机没动过 → 本机也删
    let (decision, _) = decide(
        false,
        Some(&local("h1", 100)),
        None,
        Some(&stamp("h1", 100, "aaa")),
        Evidence {
            cloud: CloudSays::Gone,
            trashed: false,
        },
    );
    assert_eq!(decision, Decision::DeleteLocal);

    // 云端删了、本机**改过** → 不跟着删（本机那份是新的，传上去）
    let (decision, _) = decide(
        false,
        Some(&local("h2", 300)),
        None,
        Some(&stamp("h1", 100, "aaa")),
        Evidence {
            cloud: CloudSays::Gone,
            trashed: false,
        },
    );
    assert_eq!(
        decision,
        Decision::Upload,
        "本机改过的那一份不能被云端的删除带走"
    );
}
