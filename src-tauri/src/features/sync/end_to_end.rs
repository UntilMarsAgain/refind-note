//! **端到端**：拿一个内存里的假 S3（只认那几个方法）把整条流程走一遍 ——
//! 传上去、把本机删干净、再从云端拿回来。签名另有官方向量盯着，这里验的是"这条路通不通"。

use super::*;
use crate::features::sync::lock::{acquire, LockBody};
use crate::features::sync::rules::{decide, Decision, Evidence, Local, Remote};
use crate::features::sync::settings::{Stamp, SyncSettings};
use crate::storage::s3::{S3Config, S3};
use crate::storage::workspace::Workspace;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use time::OffsetDateTime;

/// 桶里的东西：键 → (内容, ETag)
type Bucket = Arc<Mutex<HashMap<String, (Vec<u8>, String)>>>;

/// 一个假服务：桶 + 开关
#[derive(Clone)]
struct Fake {
    bucket: Bucket,
    /// 让**列举**装死（东西还在，就是列不出来）。
    ///
    /// 这一条是从真事上来的：桶名被填进端点（`https://桶.s3.某云.net`）、
    /// 客户端又按"桶名走路径"拼 URL，上传把桶名写进了键，列举则永远列不到东西 ——
    /// 于是"云端一片空白"被读成"云端删过"，把本机的东西删了。
    blind_listing: Arc<std::sync::atomic::AtomicBool>,
}

/// 起一个只够测试用的 S3，返回 (地址, 假服务)
fn start_fake_s3() -> (String, Fake) {
    let fake = Fake {
        bucket: Arc::new(Mutex::new(HashMap::new())),
        blind_listing: Arc::new(std::sync::atomic::AtomicBool::new(false)),
    };
    let listener = TcpListener::bind("127.0.0.1:0").expect("绑定本地端口");
    let address = format!("http://{}", listener.local_addr().unwrap());

    let served = fake.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let _ = serve(stream, &served);
        }
    });
    (address, fake)
}

/// 一次请求的应答：状态码、该带的头、正文
struct Reply {
    status: u16,
    content_type: String,
    /// 这一份的 ETag（**GET 与 LIST 必须说同一个** —— 引擎就是靠它判断
    /// "云端变没变"，两个接口说法不一致会让它以为每次都变了）
    etag: Option<String>,
    body: Vec<u8>,
}

fn serve(mut stream: TcpStream, fake: &Fake) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);

    // 请求行
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let target = parts.next().unwrap_or_default().to_string();

    // 头
    let mut headers: HashMap<String, String> = HashMap::new();
    loop {
        let mut header = String::new();
        reader.read_line(&mut header)?;
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_string());
        }
    }

    // 正文
    let length: usize = headers
        .get("content-length")
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    let mut body = vec![0u8; length];
    if length > 0 {
        reader.read_exact(&mut body)?;
    }

    let (path, query) = match target.split_once('?') {
        Some((path, query)) => (path.to_string(), query.to_string()),
        None => (target.clone(), String::new()),
    };
    let key = decode(path.trim_start_matches('/'));
    let key = key
        .split_once('/')
        .map(|(_, rest)| rest.to_string())
        .unwrap_or(key);

    let reply = route(&method, &key, &query, &headers, body, fake);

    let mut out = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n",
        reply.status,
        if reply.status == 200 { "OK" } else { "Error" },
        reply.content_type,
        reply.body.len()
    );
    if let Some(etag) = &reply.etag {
        out.push_str(&format!("ETag: \"{etag}\"\r\n"));
    }
    out.push_str("Last-Modified: Wed, 01 Oct 2026 10:00:00 GMT\r\n\r\n");
    stream.write_all(out.as_bytes())?;
    stream.write_all(&reply.body)?;
    Ok(())
}

/// 按方法分派（桶里的东西就是这个假服务的全部"状态"）
fn route(
    method: &str,
    key: &str,
    query: &str,
    headers: &HashMap<String, String>,
    body: Vec<u8>,
    fake: &Fake,
) -> Reply {
    let mut map = fake.bucket.lock().unwrap();
    // 假服务里 ETag 只要"内容一样就一样、变了就变"即可，够对账用了
    let etag_of = |bytes: &[u8]| {
        use sha2::{Digest, Sha256};
        let digest = Sha256::digest(bytes);
        digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };

    match method {
        "PUT" => {
            // 条件写：`If-None-Match: *` 只在不认识这个键时才写得进去
            if headers.get("if-none-match").map(|v| v.as_str()) == Some("*")
                && map.contains_key(key)
            {
                return Reply {
                    status: 412,
                    content_type: "text/plain".into(),
                    etag: None,
                    body: vec![],
                };
            }
            let etag = etag_of(&body);
            map.insert(key.to_string(), (body, etag.clone()));
            Reply {
                status: 200,
                content_type: "text/plain".into(),
                etag: Some(etag),
                body: vec![],
            }
        }
        "GET" if query.contains("list-type=2") => {
            // 装死：东西都在，就是列不出来（见 `Fake::blind_listing`）
            if fake.blind_listing.load(std::sync::atomic::Ordering::SeqCst) {
                return Reply {
                    status: 200,
                    content_type: "application/xml".into(),
                    etag: None,
                    body: b"<?xml version=\"1.0\"?><ListBucketResult><IsTruncated>false</IsTruncated></ListBucketResult>".to_vec(),
                };
            }
            let prefix = query
                .split('&')
                .find_map(|pair| pair.strip_prefix("prefix="))
                .map(decode)
                .unwrap_or_default();
            let mut xml = String::from("<?xml version=\"1.0\"?><ListBucketResult>");
            xml.push_str("<IsTruncated>false</IsTruncated>");
            for (name, (content, etag)) in map.iter() {
                if !name.starts_with(&prefix) {
                    continue;
                }
                // 故意把清单里的 ETag 写成**大写、不带引号**：真有的服务就是这么
                // 不讲究（清单一路说法、GET/HEAD 另一路）—— 引擎不该因此每趟
                // 都判"云端变了"，把整份仓库重下一遍。
                xml.push_str(&format!(
                    "<Contents><Key>{}</Key><LastModified>2026-10-01T10:00:00.000Z</LastModified><ETag>{}</ETag><Size>{}</Size></Contents>",
                    escape(name),
                    etag.to_uppercase(),
                    content.len()
                ));
            }
            xml.push_str("</ListBucketResult>");
            Reply {
                status: 200,
                content_type: "application/xml".into(),
                etag: None,
                body: xml.into_bytes(),
            }
        }
        "GET" => match map.get(key) {
            Some((content, etag)) => Reply {
                status: 200,
                content_type: "application/octet-stream".into(),
                etag: Some(etag.clone()),
                body: content.clone(),
            },
            None => Reply {
                status: 404,
                content_type: "text/plain".into(),
                etag: None,
                body: vec![],
            },
        },
        "HEAD" => match map.get(key) {
            Some((content, etag)) => Reply {
                status: 200,
                content_type: "application/octet-stream".into(),
                etag: Some(etag.clone()),
                body: content.clone(),
            },
            None => Reply {
                status: 404,
                content_type: "text/plain".into(),
                etag: None,
                body: vec![],
            },
        },
        "DELETE" => {
            map.remove(key);
            Reply {
                status: 204,
                content_type: "text/plain".into(),
                etag: None,
                body: vec![],
            }
        }
        _ => Reply {
            status: 400,
            content_type: "text/plain".into(),
            etag: None,
            body: vec![],
        },
    }
}

fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let high = (bytes[index + 1] as char).to_digit(16);
            let low = (bytes[index + 2] as char).to_digit(16);
            if let (Some(high), Some(low)) = (high, low) {
                out.push((high * 16 + low) as u8);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;")
}

/// 一个干净的工作目录（只有该同步的那些文件）
fn workspace(name: &str) -> Workspace {
    let dir = std::env::temp_dir().join(format!(
        "refind-note-sync-test-{}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let workspace = Workspace::open(dir).unwrap();
    // db 目录得先在（真正的程序是打开数据库时建的）
    std::fs::create_dir_all(workspace.root().join("db/objects/0")).unwrap();
    std::fs::create_dir_all(workspace.root().join("db/blobs/ab")).unwrap();
    std::fs::create_dir_all(workspace.root().join("db/drafts")).unwrap();
    std::fs::create_dir_all(workspace.root().join("settings")).unwrap();
    workspace
}

fn settings_for(address: &str, key: &str) -> SyncSettings {
    SyncSettings {
        enabled: true,
        encrypt: true,
        cipher: crate::storage::codec::Cipher::Aes256Gcm,
        reupload: false,
        key: key.to_string(),
        s3: S3Config {
            endpoint: address.to_string(),
            region: "us-east-1".to_string(),
            bucket: "bucket".to_string(),
            prefix: "notes".to_string(),
            access_key: "AK".to_string(),
            secret_key: "SK".to_string(),
        },
    }
}

/// **用户在意的那个流程**：写点东西 → 传上去 → 把本机删干净 → 换台机器拿回来
#[test]
fn a_repository_travels_to_the_cloud_and_comes_back() {
    let (address, _bucket) = start_fake_s3();
    let key = generate_key().unwrap();

    // ---- 甲机器：写了两样东西，传上去 ----
    let first = workspace("push");
    std::fs::write(
        first.root().join("db/objects/0/1.log"),
        "{\"rev\":1}\n".as_bytes(),
    )
    .unwrap();
    std::fs::write(first.root().join("db/blobs/ab/abc"), b"PNGDATA").unwrap();
    std::fs::write(first.root().join("settings/repository.json"), b"{\"x\":1}").unwrap();

    let settings = settings_for(&address, &key);
    let report = run(&first, &settings, &|_| {}, false).unwrap();
    assert_eq!(report.uploaded, 3, "三份都该传上去：{report:?}");
    assert_eq!(report.downloaded, 0);

    // 本机自己的东西不该上去
    std::fs::write(first.root().join("settings/preferences.json"), b"{}").unwrap();
    std::fs::write(first.root().join("db/drafts/1"), "写了一半".as_bytes()).unwrap();
    let report = run(&first, &settings, &|_| {}, false).unwrap();
    assert_eq!(
        report.uploaded + report.downloaded,
        0,
        "没改过就什么都不动：{report:?}"
    );

    // ---- 把本机删干净（等于换台机器） ----
    let second = workspace("pull");
    assert!(!second.root().join("db/objects/0/1.log").exists());

    // ---- 乙机器：配置同一个桶、同一把钥匙，同步一次 ----
    let report = run(&second, &settings, &|_| {}, false).unwrap();
    assert_eq!(report.downloaded, 3, "三份都该拿回来：{report:?}");
    assert_eq!(
        std::fs::read_to_string(second.root().join("db/objects/0/1.log")).unwrap(),
        "{\"rev\":1}\n"
    );
    assert_eq!(
        std::fs::read(second.root().join("db/blobs/ab/abc")).unwrap(),
        b"PNGDATA"
    );
    assert_eq!(
        std::fs::read_to_string(second.root().join("settings/repository.json")).unwrap(),
        "{\"x\":1}"
    );

    // 乙机器上没配同步时写下的本机专属文件，不该被传上去
    let report = run(&second, &settings, &|_| {}, false).unwrap();
    assert_eq!(
        report.uploaded + report.downloaded,
        0,
        "第二次同步应当无事可做：{report:?}"
    );

    let _ = std::fs::remove_dir_all(first.root());
    let _ = std::fs::remove_dir_all(second.root());
}

/// 钥匙不对：说得清是钥匙的问题（而不是"文件坏了"）
#[test]
fn the_wrong_key_says_so() {
    let (address, _bucket) = start_fake_s3();

    let first = workspace("key-a");
    std::fs::write(first.root().join("db/objects/0/1.log"), b"secret").unwrap();
    let settings = settings_for(&address, &generate_key().unwrap());
    run(&first, &settings, &|_| {}, false).unwrap();

    // 换台机器、抄错了钥匙
    let second = workspace("key-b");
    let wrong = settings_for(&address, &generate_key().unwrap());
    let error = run(&second, &wrong, &|_| {}, false).unwrap_err();
    assert!(error.contains("密钥"), "{error}");
    assert!(!second.root().join("db/objects/0/1.log").exists());

    let _ = std::fs::remove_dir_all(first.root());
    let _ = std::fs::remove_dir_all(second.root());
}

/// 两台机器都改过同一份：按时间取新的那份，并且**报告里说一声**
#[test]
fn a_conflict_is_reported_not_swallowed() {
    let (address, _bucket) = start_fake_s3();
    let key = generate_key().unwrap();

    let first = workspace("conflict-a");
    std::fs::write(first.root().join("db/titles.json"), b"{\"from\":\"a\"}").unwrap();
    let settings = settings_for(&address, &key);
    run(&first, &settings, &|_| {}, false).unwrap();

    let second = workspace("conflict-b");
    run(&second, &settings, &|_| {}, false).unwrap();

    // 乙机器改成"本机这份不旧"的样子（时间往后挪一点，甲机器那份会更旧）
    std::fs::write(second.root().join("db/titles.json"), b"{\"from\":\"b\"}").unwrap();
    // 同时让云端那份也变（拿"本地改过、云端改过"这条分支）
    let third = workspace("conflict-c");
    run(&third, &settings, &|_| {}, false).unwrap();
    std::fs::write(third.root().join("db/titles.json"), b"{\"from\":\"c\"}").unwrap();
    run(&third, &settings, &|_| {}, false).unwrap();

    let report = run(&second, &settings, &|_| {}, false).unwrap();
    assert_eq!(report.conflicts.len(), 1, "两边都改过要报出来：{report:?}");
    assert_eq!(report.conflicts[0].path, "db/titles.json");

    let _ = std::fs::remove_dir_all(first.root());
    let _ = std::fs::remove_dir_all(second.root());
    let _ = std::fs::remove_dir_all(third.root());
}

/// 两边的 ETag 只差大小写不算"变了" —— 不然就是每趟全量重下。
///
/// 这一条是从真事上来的：清单与 GET/HEAD 是两条路，服务对同一个 ETag 的写法
/// 偶尔不一致，而"云端变没变"全靠它比。
#[test]
fn an_etag_that_only_differs_in_case_is_not_a_change() {
    let stamp = Stamp {
        hash: "h".to_string(),
        mtime: 100,
        etag: "9F2C4A".to_string(),
    };
    let here = Local {
        hash: "h".to_string(),
        mtime: 100,
    };
    let there = Remote {
        size: 10,
        etag: "9f2c4a".to_string(),
        modified: 100,
    };
    let (decision, _) = decide(
        false,
        Some(&here),
        Some(&there),
        Some(&stamp),
        Evidence::none(),
    );
    assert_eq!(decision, Decision::Nothing, "只差大小写不算云端变了");
}

/// 云端那一层认设置里的算法：选了国密，传上去的就是国密的封装
///
/// 顺带钉住一件要紧的事：**换算法不必换钥匙、也不必重传** ——
/// 每一份封装的头里记着自己那一档，换回 AES 之后老对象照旧解得开。
#[test]
fn the_cloud_layer_uses_the_chosen_cipher() {
    let (address, bucket) = start_fake_s3();
    let key = generate_key().unwrap();

    let first = workspace("cipher-sm4");
    std::fs::write(
        first.root().join("db/objects/0/1.log"),
        "{\"rev\":1}\n".as_bytes(),
    )
    .unwrap();

    let mut settings = settings_for(&address, &key);
    settings.cipher = crate::storage::codec::Cipher::Sm4Gcm;
    assert_eq!(
        settings.view().cipher,
        crate::storage::codec::Cipher::Sm4Gcm
    );
    run(&first, &settings, &|_| {}, false).unwrap();

    // 云端那份的封装头上记着用的是哪一档（magic(4) + 版本(1) + 算法(1)，见 CloudEnvelope）
    let stored = bucket.bucket.lock().unwrap()["notes/db/objects/0/1.log"]
        .0
        .clone();
    assert_eq!(&stored[..4], b"RNDS", "应当是封装过的");
    assert_eq!(stored[5], 2, "算法那一字节：2 = 国密 SM4");

    // 换台机器、换回 AES：老那份（国密的）照旧解得开
    let second = workspace("cipher-aes");
    let mut settings = settings_for(&address, &key);
    settings.cipher = crate::storage::codec::Cipher::Aes256Gcm;
    let report = run(&second, &settings, &|_| {}, false).unwrap();
    assert_eq!(report.downloaded, 1, "该把那一份拿回来：{report:?}");
    assert_eq!(
        std::fs::read_to_string(second.root().join("db/objects/0/1.log")).unwrap(),
        "{\"rev\":1}\n"
    );

    let _ = std::fs::remove_dir_all(first.root());
    let _ = std::fs::remove_dir_all(second.root());
}

/// **清单一片空白时，什么都不能删** —— 这是真出过的事故（端到端复现）。
///
/// 用户那台机器上：桶名被填进了端点、客户端又按"桶名走路径"拼 URL，
/// 于是东西都传上去了（PUT/HEAD 自洽，看着成功），可**列举**永远列不到 ——
/// 每同步一次，就把本机"账上记着已对齐"的文件当成"云端删过的"删掉一批。
#[test]
fn a_blind_listing_deletes_nothing_at_all() {
    let (address, fake) = start_fake_s3();
    let key = generate_key().unwrap();
    let machine = workspace("blind");
    std::fs::write(
        machine.root().join("db/objects/0/1.log"),
        "{\"rev\":1}\n".as_bytes(),
    )
    .unwrap();
    std::fs::write(machine.root().join("db/blobs/ab/abc"), b"PNGDATA").unwrap();
    let settings = settings_for(&address, &key);

    // 第一趟：正常传上去（账本也写上去）
    let report = run(&machine, &settings, &|_| {}, false).unwrap();
    assert_eq!(report.uploaded + report.downloaded, 2, "{report:?}");

    // 第二趟：列举装死（东西其实都在桶里）
    fake.blind_listing
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let report = run(&machine, &settings, &|_| {}, false).unwrap();

    assert!(
        machine.root().join("db/objects/0/1.log").is_file(),
        "本机这一份必须还在：{report:?}"
    );
    assert!(
        machine.root().join("db/blobs/ab/abc").is_file(),
        "内容块也要还在：{report:?}"
    );
    assert_eq!(report.removed_local, 0, "不许删本机：{report:?}");
    assert_eq!(report.removed_remote, 0, "也不许删云端：{report:?}");

    let _ = std::fs::remove_dir_all(machine.root());
}

/// 新机器（本地没有账本）照着云端那份认路：不重传，也不乱删
#[test]
fn a_fresh_machine_learns_from_the_cloud_ledger() {
    let (address, fake) = start_fake_s3();
    let key = generate_key().unwrap();

    let first = workspace("ledger-a");
    std::fs::write(
        first.root().join("db/objects/0/1.log"),
        "{\"rev\":1}\n".as_bytes(),
    )
    .unwrap();
    let settings = settings_for(&address, &key);
    run(&first, &settings, &|_| {}, false).unwrap();

    // 账本确实写到云端了
    assert!(
        fake.bucket
            .lock()
            .unwrap()
            .keys()
            .any(|name| name.ends_with(LATEST_KEY)),
        "跑完一趟应当留下账本"
    );

    // 甲机器再同步一次：账本在，什么都不用动
    let report = run(&first, &settings, &|_| {}, false).unwrap();
    assert_eq!(
        report.uploaded + report.downloaded,
        0,
        "账本在手，第二次该无事可做：{report:?}"
    );

    // 乙机器：把甲那份仓库整个抄过来，**唯独不带本地账本**（等于新装的机器）
    let second = workspace("ledger-b");
    std::fs::copy(
        first.root().join("db/objects/0/1.log"),
        second.root().join("db/objects/0/1.log"),
    )
    .unwrap();
    assert!(!second.settings_file("sync-index.json").exists());

    let report = run(&second, &settings, &|_| {}, false).unwrap();
    assert_eq!(
        report.uploaded + report.downloaded + report.removed_local,
        0,
        "照云端的账认路之后：不重传、不下载、更不删：{report:?}"
    );

    let _ = std::fs::remove_dir_all(first.root());
    let _ = std::fs::remove_dir_all(second.root());
}

/// 云端那把锁还热着：普通同步让你等着，**强制同步**现在就抢过来。
///
/// 对应界面上那颗「强制同步」—— 另一边崩在半路时，不必干等它超时。
#[test]
fn a_forced_sync_takes_a_lock_that_is_still_warm() {
    let (address, bucket) = start_fake_s3();
    let settings = settings_for(&address, "");
    let s3 = S3::new(settings.s3.clone()).unwrap();
    let key = s3.config().key_of(LOCK_KEY);

    // 另一台机器刚拿的锁：时间戳是现在，离过期还早
    let held = serde_json::to_vec(&LockBody {
        host: "另一台机器".to_string(),
        at: OffsetDateTime::now_utc().unix_timestamp(),
    })
    .unwrap();
    bucket
        .bucket
        .lock()
        .unwrap()
        .insert(key.clone(), (held, "held".to_string()));

    // 普通同步：不动手，并且说清是锁挡着
    let polite = match acquire(&s3, false) {
        Ok(_) => panic!("锁还热着的时候不该动手"),
        Err(message) => message,
    };
    assert!(polite.contains("锁"), "该说清是锁挡着：{polite}");
    assert!(
        bucket.bucket.lock().unwrap().contains_key(&key),
        "锁还该在原处"
    );

    // 强制：抢过来，锁里换成我们的名字
    let lock = acquire(&s3, true).expect("强制同步应当拿到锁");
    let holder: LockBody = serde_json::from_slice(&bucket.bucket.lock().unwrap()[&key].0).unwrap();
    assert_ne!(holder.host, "另一台机器", "锁该换成我们拿着了");

    // 放掉之后云端不留锁，下一次谁都能同步
    lock.release(&s3).unwrap();
    assert!(!bucket.bucket.lock().unwrap().contains_key(&key));
}
