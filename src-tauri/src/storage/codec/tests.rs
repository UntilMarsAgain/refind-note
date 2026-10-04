//! 封装格式的测试。
//!
//! `use super::*` 之后能摸到编排层的公开部分（`encode` / `decode` / `inspect` 与
//! 各份报告、类型）—— 测试大多走的就是这条正路。要碰各层内部那几把私有钥匙时，
//! 得从对应子模块单独导入：`use super::*` 不会带出**孙模块**的项。

use super::gpg::FORCE_NO_GPG;
use super::symmetric::{open_with, seal_with};
use super::*;

/// 算法在 JSON 里怎么写：界面（`ipc/note.ts`）按这串字选，写错了就是一句
/// "unknown variant `aes-256-gcm`"。老文件里是 `aes256-gcm`（少一个横线），
/// 也要能读回来 —— 那正是这条测试存在的原因。
#[test]
fn a_cipher_spells_the_same_in_json_and_in_the_interface() {
    assert_eq!(
        serde_json::to_string(&Cipher::Aes256Gcm).unwrap(),
        "\"aes-256-gcm\""
    );
    assert_eq!(
        serde_json::to_string(&Cipher::Sm4Gcm).unwrap(),
        "\"sm4-gcm\""
    );

    for (text, wanted) in [
        ("\"aes-256-gcm\"", Cipher::Aes256Gcm),
        ("\"aes256-gcm\"", Cipher::Aes256Gcm),
        ("\"sm4-gcm\"", Cipher::Sm4Gcm),
        ("\"sm4gcm\"", Cipher::Sm4Gcm),
    ] {
        assert_eq!(
            serde_json::from_str::<Cipher>(text).unwrap(),
            wanted,
            "从 {text} 读出来"
        );
    }
}

/// gpg 的可用性是进程级开关，碰它的用例要串行。
fn gpg_guard() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[test]
fn no_layers_is_wrapped_but_readable() {
    // 什么都不做也要有头："裸字节"只是"空链"，不存在"有时有头有时没头"
    let file = encode(b"hello", &Meta::default(), &Policy::default(), None).unwrap();

    assert_eq!(&file[..4], MAGIC);
    assert!(inspect(&file).unwrap().protection.is_plain());
    assert_eq!(decode(&file, &Secrets::default()).unwrap(), b"hello");
}

#[test]
fn compression_round_trips() {
    let content = "重复的内容。".repeat(200);
    let policy = Policy {
        compress: true,
        ..Default::default()
    };

    let file = encode(content.as_bytes(), &Meta::default(), &policy, None).unwrap();
    assert!(file.len() < content.len(), "压缩该让体积变小");
    assert!(inspect(&file).unwrap().protection.compress);
    assert_eq!(
        decode(&file, &Secrets::default()).unwrap(),
        content.as_bytes()
    );
}

/// 两档压缩算法都转得回来，而且**头里记着用的是哪一档**
#[test]
fn both_compression_algorithms_round_trip() {
    let content = "重复的内容。".repeat(200);

    for algorithm in [Compression::Deflate, Compression::Brotli] {
        let policy = Policy {
            compress: true,
            compression: algorithm,
            ..Default::default()
        };
        let file = encode(content.as_bytes(), &Meta::default(), &policy, None).unwrap();

        let inspection = inspect(&file).unwrap();
        assert!(inspection.protection.compress);
        assert_eq!(inspection.protection.compression, algorithm);

        // 头里也真的写着（换台机器读，靠的就是它）
        let header = read_header(&file).unwrap();
        assert!(matches!(
            header.layers[0],
            Layer::Compress { algorithm: found, .. } if found == algorithm
        ));

        assert!(file.len() < content.len(), "压缩该让体积变小");
        assert_eq!(
            decode(&file, &Secrets::default()).unwrap(),
            content.as_bytes()
        );
    }
}

/// 密文要**与别家实现互通**：拿公开的测试向量对一遍（AES 那组来自 NIST，
/// SM4 那组来自 BouncyCastle）。国密那一档的意义就是"换个实现也解得开"。
#[test]
fn both_ciphers_match_published_test_vectors() {
    let hex = |bytes: &[u8]| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();

    // AES-256-GCM：密钥全 0、nonce 全 0、明文全 0 的 16 字节
    let sealed = seal_with(Cipher::Aes256Gcm, &[0u8; 32], &[0u8; 12], &[0u8; 16]).unwrap();
    assert_eq!(
        hex(&sealed),
        "cea7403d4d606b6e074ec5d3baf39d18d0d1c8a799996bf0265b98b5d48ab919"
    );
    assert_eq!(
        open_with(Cipher::Aes256Gcm, &[0u8; 32], &[0u8; 12], &sealed).unwrap(),
        vec![0u8; 16]
    );

    // SM4-GCM：同样是全 0，用的是 BouncyCastle 那组向量
    let sealed = seal_with(Cipher::Sm4Gcm, &[0u8; 32], &[0u8; 12], b"hello world").unwrap();
    assert_eq!(
        hex(&sealed),
        "1587c6137e306fed6a6a5f49539b6dd6fe2b7872c3279636db07c2"
    );
    assert_eq!(
        open_with(Cipher::Sm4Gcm, &[0u8; 32], &[0u8; 12], &sealed).unwrap(),
        b"hello world".to_vec()
    );
}

/// 两档口令算法都转得回来；换个算法解不开（头说了算，不是当前设置说了算）
#[test]
fn both_ciphers_round_trip() {
    let content = b"only for us".to_vec();

    for cipher in [Cipher::Aes256Gcm, Cipher::Sm4Gcm] {
        let policy = Policy {
            symmetric: true,
            cipher,
            ..Default::default()
        };
        let file = encode(&content, &Meta::default(), &policy, Some("口令")).unwrap();

        assert_eq!(inspect(&file).unwrap().protection.cipher, cipher);
        assert_eq!(
            decode(
                &file,
                &Secrets {
                    passphrase: Some("口令")
                }
            )
            .unwrap(),
            content
        );

        // 口令不对：两档都报同一句话
        let error = decode(
            &file,
            &Secrets {
                passphrase: Some("别的"),
            },
        )
        .unwrap_err();
        assert_eq!(error, CodecError::WrongPassphrase);
    }
}

#[test]
fn symmetric_round_trips_and_needs_the_right_passphrase() {
    let content = b"only for us".to_vec();
    let policy = Policy {
        symmetric: true,
        ..Default::default()
    };

    let file = encode(&content, &Meta::default(), &policy, Some("open sesame")).unwrap();
    // 内容是看不见的
    assert!(!file.windows(11).any(|window| window == b"only for us"));

    // 头是明文：不输口令也知道这是加密的
    assert!(inspect(&file).unwrap().protection.symmetric);

    // 没给口令
    assert!(matches!(
        decode(&file, &Secrets::default()),
        Err(CodecError::PassphraseNeeded)
    ));
    // 口令不对
    assert!(matches!(
        decode(
            &file,
            &Secrets {
                passphrase: Some("wrong")
            }
        ),
        Err(CodecError::WrongPassphrase)
    ));
    // 对的口令
    assert_eq!(
        decode(
            &file,
            &Secrets {
                passphrase: Some("open sesame")
            }
        )
        .unwrap(),
        content
    );
}

#[test]
fn the_whole_chain_nests_in_the_right_order() {
    // 压缩 + 对称：先压后加，读的时候反过来
    let content = "一二三四五六七八九十".repeat(50);
    let policy = Policy {
        compress: true,
        symmetric: true,
        ..Default::default()
    };

    let file = encode(content.as_bytes(), &Meta::default(), &policy, Some("pw")).unwrap();
    let protection = inspect(&file).unwrap().protection;
    assert!(protection.compress && protection.symmetric);

    // 层链是从内到外的：压缩在内，对称在外
    let header = read_header(&file).unwrap();
    assert!(matches!(
        header.layers[0],
        Layer::Compress {
            algorithm: Compression::Deflate,
            ..
        }
    ));
    assert!(matches!(header.layers[1], Layer::Symmetric { .. }));

    assert_eq!(
        decode(
            &file,
            &Secrets {
                passphrase: Some("pw")
            }
        )
        .unwrap(),
        content.as_bytes()
    );
}

#[test]
fn a_foreign_file_is_told_apart_from_a_corrupt_one() {
    assert!(matches!(
        decode(b"not ours", &Secrets::default()),
        Err(CodecError::NotEnvelope(_))
    ));

    // 版本不认得也算"不是我们的"
    let mut file = encode(b"x", &Meta::default(), &Policy::default(), None).unwrap();
    file[4] = 99;
    assert!(matches!(inspect(&file), Err(CodecError::NotEnvelope(_))));

    // 头被截断才算损坏
    let file = encode(b"x", &Meta::default(), &Policy::default(), None).unwrap();
    assert!(matches!(
        inspect(&file[..6]),
        Err(CodecError::NotEnvelope(_))
    ));
}

#[test]
fn an_unknown_layer_is_reported_by_name() {
    // 加了新层时，老程序要说得清"是我不认识"，而不是"文件坏了"
    let header = br#"{"layers":[{"layer":"quantum","qbits":7}]}"#;
    let mut file = Vec::new();
    file.extend_from_slice(MAGIC);
    file.push(FORMAT_VERSION);
    file.extend_from_slice(&(header.len() as u32).to_be_bytes());
    file.extend_from_slice(header);
    file.extend_from_slice(b"payload");

    let error = inspect(&file).unwrap_err();
    assert_eq!(error, CodecError::UnknownLayer("quantum".to_string()));
    assert!(error.to_string().contains("quantum"), "{error}");
}

#[test]
fn the_header_carries_the_content_mime() {
    let meta = Meta {
        mime: "image/png".to_string(),
    };
    let file = encode(b"\x89PNG", &meta, &Policy::default(), None).unwrap();

    // 头是明文：不解层就知道里面是什么
    assert_eq!(read_header(&file).unwrap().meta.mime, "image/png");
    assert_eq!(inspect(&file).unwrap().meta.mime, "image/png");
    // 内容照常解出来
    assert_eq!(decode(&file, &Secrets::default()).unwrap(), b"\x89PNG");
}

#[test]
fn an_unknown_meta_key_is_ignored() {
    // meta 只是声明：认不得的字段忽略，不像未知层那样报错（它不改字节语义）
    let header = br#"{"layers":[],"meta":{"mime":"text/plain","future":7}}"#;
    let mut file = Vec::new();
    file.extend_from_slice(MAGIC);
    file.push(FORMAT_VERSION);
    file.extend_from_slice(&(header.len() as u32).to_be_bytes());
    file.extend_from_slice(header);
    file.extend_from_slice(b"payload");

    assert_eq!(inspect(&file).unwrap().meta.mime, "text/plain");
    assert_eq!(decode(&file, &Secrets::default()).unwrap(), b"payload");
}

/// gpg 那两层（签名、加密）。这一条以前没有，所以"验签参数传反了"没被发现。
///
/// 需要一个能生成密钥的 gpg：测试用**临时家目录**，不去动用户自己的钥匙串。
#[test]
fn gpg_layers_round_trip() {
    let _guard = gpg_guard();
    // 没有 gpg 的机器上，这些功能本来就不可用（那条路径另有用例钉住）
    if !gpg_available() {
        eprintln!("跳过 gpg 测试：这台计算机上没有 gpg");
        return;
    }

    let home = std::env::temp_dir().join(format!("refind-note-gpg-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).unwrap();

    let recipient = "refind-note-test@example.com";
    prepare_gpg_home(&home, recipient);
    set_gpg_home(home.clone());

    let content = b"sign me".to_vec();

    // 只签名：不改载荷，所以未加密的 blob 不需要口令也能验
    let signed = encode(
        &content,
        &Meta::default(),
        &Policy {
            gpg_sign: Some(recipient.to_string()),
            ..Default::default()
        },
        None,
    )
    .unwrap();

    let protection = inspect(&signed).unwrap().protection;
    assert!(
        protection.sign.is_some(),
        "头是明文，签名状态不该需要钥匙才看得出来"
    );
    assert_eq!(decode(&signed, &Secrets::default()).unwrap(), content);

    // 验签报告：过没过、指纹、以及本地对这枚公钥的信任程度
    let report = signature_report(&signed, &Secrets::default())
        .unwrap()
        .expect("签过的 blob 应当有报告");
    assert!(report.verified, "{report:?}");
    assert!(report.detail.contains("指纹"), "{report:?}");
    assert!(report.trust.is_some(), "{report:?}");

    // 内容被动过一个字节：报告说"没通过"，而不是抛错 —— 验签的结论与验签跑不起来要分开
    let mut broken = signed.clone();
    let last = broken.len() - 1;
    broken[last] ^= 0x01;
    let report = signature_report(&broken, &Secrets::default())
        .unwrap()
        .expect("层链没变，报告照样出得来");
    assert!(!report.verified, "{report:?}");
    assert!(!report.detail.is_empty(), "{report:?}");

    // 没签名层就没有报告
    let plain = encode(&content, &Meta::default(), &Policy::default(), None).unwrap();
    assert!(signature_report(&plain, &Secrets::default())
        .unwrap()
        .is_none());

    // 加密：载荷变了，解出来要一模一样
    let sealed = encode(
        &content,
        &Meta::default(),
        &Policy {
            gpg_encrypt: Some(recipient.to_string()),
            ..Default::default()
        },
        None,
    )
    .unwrap();
    assert!(!sealed
        .windows(content.len())
        .any(|window| window == content));
    assert!(inspect(&sealed).unwrap().protection.encrypt.is_some());
    assert_eq!(decode(&sealed, &Secrets::default()).unwrap(), content);

    // 签名 + 压缩 + 加密，三层一起
    let whole = encode(
        &content,
        &Meta::default(),
        &Policy {
            compress: true,
            gpg_sign: Some(recipient.to_string()),
            gpg_encrypt: Some(recipient.to_string()),
            ..Default::default()
        },
        None,
    )
    .unwrap();
    assert_eq!(decode(&whole, &Secrets::default()).unwrap(), content);

    let _ = std::fs::remove_dir_all(&home);
}

/// 信任程度的人话映射。它不碰钥匙串，所以不必等 gpg
#[test]
fn trust_labels_say_what_they_mean() {
    assert_eq!(trust_label(gpgme::Validity::Unknown), "本机没有这把公钥");
    assert_eq!(trust_label(gpgme::Validity::Full), "完全信任");
    assert_eq!(trust_label(gpgme::Validity::Ultimate), "绝对信任");
}

/// 没有 gpg 时：这些功能**不可用**，给一句说得清的错，而不是底层报错。
#[test]
fn without_gpg_the_gpg_features_report_unavailable() {
    let _guard = gpg_guard();

    FORCE_NO_GPG.store(true, std::sync::atomic::Ordering::Relaxed);
    let signed = encode(
        b"x",
        &Meta::default(),
        &Policy {
            gpg_sign: Some("谁".to_string()),
            ..Default::default()
        },
        None,
    );
    let sealed = encode(
        b"x",
        &Meta::default(),
        &Policy {
            gpg_encrypt: Some("谁".to_string()),
            ..Default::default()
        },
        None,
    );
    FORCE_NO_GPG.store(false, std::sync::atomic::Ordering::Relaxed);

    for result in [signed, sealed] {
        let error = result.unwrap_err();
        assert_eq!(error, CodecError::GpgUnavailable);
        assert!(error.to_string().contains("没有 gpg"), "{error}");
    }
}

/// 建一个临时的 gpg 家目录，并生成一把**无口令**的测试密钥。
///
/// 测试环境里没有交互，所以只能这么办；没生成出来就直接失败 ——
/// 静默跳过会让"签名坏了"这件事重新藏起来。
fn prepare_gpg_home(home: &std::path::Path, user: &str) {
    let status = std::process::Command::new("gpg")
        .arg("--homedir")
        .arg(home)
        .args([
            "--batch",
            "--pinentry-mode",
            "loopback",
            "--passphrase",
            "",
            "--quick-generate-key",
            user,
            "default",
            "default",
            "never",
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    match status {
        // 连 gpg 都没装：这条路径无从测起
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("跳过 gpg 测试：找不到 gpg 命令");
        }
        Ok(status) if status.success() => {}
        other => panic!("生成 gpg 测试密钥失败：{other:?}"),
    }
}

#[test]
fn the_address_of_a_signed_blob_changes_with_the_signature() {
    // 签名不改载荷，但签名本身进头 → 落盘字节不同 → 地址不同。
    // 这正是"签过的文件不该被当成同一份"的落实。
    let layers = |signature: &str| {
        vec![Layer::Gpg {
            mode: GpgMode::Sign,
            key: "ABC".to_string(),
            signature: Some(signature.to_string()),
        }]
    };
    let first = assemble(
        &layers("c2lnbmF0dXJlLTE="),
        &Meta::default(),
        b"same payload".to_vec(),
    )
    .unwrap();
    let second = assemble(
        &layers("c2lnbmF0dXJlLTI="),
        &Meta::default(),
        b"same payload".to_vec(),
    )
    .unwrap();

    assert_ne!(first, second);
    assert_ne!(
        crate::storage::store::hash_hex(&first),
        crate::storage::store::hash_hex(&second)
    );
}
