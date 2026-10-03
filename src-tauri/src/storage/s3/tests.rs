//! S3 客户端的自测：签名的官方测试向量、路径与编码的规矩、清单与 ETag 的读法。

use super::*;

/// 端点里**带了桶名**时，URL 里不该再补一段。
///
/// 这一条是从真事上来的：补了那一段，东西就存成 `桶名/db/…`
/// （上传 PUT 与 HEAD 走同一条路，自洽，看着像成功了），而列举——列举也走这条路——
/// 永远列不到那些键。接着"云端一片空白"被读成"云端删过"，把本机的东西删了。
#[test]
fn a_bucket_in_the_hostname_is_not_repeated_in_the_path() {
    let with_bucket = S3Config {
        endpoint: "https://uma-refind.s3.bitiful.net".to_string(),
        bucket: "uma-refind".to_string(),
        access_key: "AK".to_string(),
        secret_key: "SK".to_string(),
        ..Default::default()
    };
    let client = S3::new(with_bucket).unwrap();
    assert_eq!(client.path_of("db/meta.json"), "/db/meta.json");
    assert_eq!(client.path_of(""), "/", "列举就去桶根上列");

    // 域名里没有桶（自建的 MinIO、AWS 的通用端点）→ 照旧把桶名补进路径
    let plain = S3Config {
        endpoint: "http://127.0.0.1:9000".to_string(),
        bucket: "notes".to_string(),
        access_key: "AK".to_string(),
        secret_key: "SK".to_string(),
        ..Default::default()
    };
    assert_eq!(S3::new(plain).unwrap().path_of("db/meta.json"), "/notes/db/meta.json");
}

/// 同一个 ETag 的几种写法要读成同一个值 —— 清单走 XML、GET/HEAD 走响应头。
///
/// 读出来不一样是什么下场：每趟都判"云端变了"，下载完再同步又是全量下载。
#[test]
fn an_etag_reads_the_same_however_it_is_spelled() {
    for raw in [
        "abc",
        "\"abc\"",
        "&quot;abc&quot;",
        "&#34;abc&#34;",
        "  \"abc\"  ",
    ] {
        assert_eq!(clean_etag(raw), "abc", "从 {raw} 里读出来");
    }
}

/// **官方测试向量**：AWS 文档里那个 S3 GET 的例子，逐字节对签名。
/// 对上了就说明与别家实现互通（MinIO / R2 / OSS 认的都是这一套）。
#[test]
fn sigv4_matches_the_published_example() {
    let headers = vec![
        (
            "host".to_string(),
            "examplebucket.s3.amazonaws.com".to_string(),
        ),
        ("range".to_string(), "bytes=0-9".to_string()),
        (
            "x-amz-content-sha256".to_string(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".to_string(),
        ),
        ("x-amz-date".to_string(), "20130524T000000Z".to_string()),
    ];
    let authorization = sign(&SigningInput {
        method: "GET",
        path: "/test.txt",
        query: "",
        headers: &headers,
        payload_hash: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        stamp: "20130524T000000Z",
        date: "20130524",
        region: "us-east-1",
        access_key: "AKIAIOSFODNN7EXAMPLE",
        secret_key: "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY",
    });

    assert_eq!(
        authorization,
        "AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/20130524/us-east-1/s3/aws4_request, \
         SignedHeaders=host;range;x-amz-content-sha256;x-amz-date, \
         Signature=f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41"
            .replace("             ", "")
    );
}

/// 官方那个 PUT 的例子（带 `x-amz-storage-class`、正文有内容）
#[test]
fn sigv4_matches_the_put_example() {
    let body_hash = hex(&Sha256::digest(b"Welcome to Amazon S3."));
    let headers = vec![
        (
            "date".to_string(),
            "Fri, 24 May 2013 00:00:00 GMT".to_string(),
        ),
        (
            "host".to_string(),
            "examplebucket.s3.amazonaws.com".to_string(),
        ),
        ("x-amz-content-sha256".to_string(), body_hash.clone()),
        ("x-amz-date".to_string(), "20130524T000000Z".to_string()),
        (
            "x-amz-storage-class".to_string(),
            "REDUCED_REDUNDANCY".to_string(),
        ),
    ];
    let authorization = sign(&SigningInput {
        method: "PUT",
        path: "/test%24file.text",
        query: "",
        headers: &headers,
        payload_hash: &body_hash,
        stamp: "20130524T000000Z",
        date: "20130524",
        region: "us-east-1",
        access_key: "AKIAIOSFODNN7EXAMPLE",
        secret_key: "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY",
    });

    assert_eq!(
        authorization,
        "AWS4-HMAC-SHA256 Credential=AKIAIOSFODNN7EXAMPLE/20130524/us-east-1/s3/aws4_request, \
         SignedHeaders=date;host;x-amz-content-sha256;x-amz-date;x-amz-storage-class, \
         Signature=98ad721746da40c64f1a55b78f14c238d841ea1380cd77a1b5971af0ece108bd"
            .replace("             ", "")
    );
}

#[test]
fn paths_are_percent_encoded_the_sigv4_way() {
    // 空格写 %20（不是 `+`），`/` 留着分段
    assert_eq!(
        encode_path("db/blobs/ab/桥 图.png"),
        "db/blobs/ab/%E6%A1%A5%20%E5%9B%BE.png"
    );
    assert_eq!(encode("a-b_c.d~e"), "a-b_c.d~e");
    assert_eq!(encode("a/b"), "a%2Fb");
}

#[test]
fn a_prefix_joins_into_the_object_key() {
    let config = S3Config {
        prefix: "notes/".to_string(),
        ..Default::default()
    };
    assert_eq!(config.key_of("db/titles.json"), "notes/db/titles.json");

    let bare = S3Config::default();
    assert_eq!(bare.key_of("db/titles.json"), "db/titles.json");
}

#[test]
fn the_endpoint_host_is_taken_for_signing() {
    assert_eq!(host_of("https://s3.example.com"), "s3.example.com");
    assert_eq!(host_of("http://127.0.0.1:9000"), "127.0.0.1:9000");
    assert_eq!(host_of("https://s3.example.com/"), "s3.example.com");
}

/// 清单的 XML：抠得出键、大小、ETag，也认得出"还有下一页"
#[test]
fn the_list_response_is_read() {
    let xml = r#"<?xml version="1.0"?>
<ListBucketResult>
  <IsTruncated>true</IsTruncated>
  <NextContinuationToken>1ueGcxLPRx1Tr</NextContinuationToken>
  <Contents><Key>notes/db/&#xE6;&#x8C;&#xA4;.png</Key><LastModified>2026-10-01T10:00:00.000Z</LastModified><ETag>&quot;abc&quot;</ETag><Size>12</Size></Contents>
  <Contents><Key>a&amp;b.log</Key><LastModified>2026-10-01T11:00:00.000Z</LastModified><ETag>&quot;def&quot;</ETag><Size>34</Size></Contents>
</ListBucketResult>"#;
    let page = parse_list(xml).unwrap();

    assert_eq!(page.objects.len(), 2);
    assert_eq!(page.objects[0].size, 12);
    assert_eq!(page.objects[0].etag, "abc");
    assert_eq!(page.objects[1].key, "a&b.log");
    assert_eq!(page.next_token.as_deref(), Some("1ueGcxLPRx1Tr"));
}

/// 没截断就不该有下一页（有些服务的 XML 里留着空标签，别当真）
#[test]
fn a_finished_list_has_no_next_page() {
    let xml = "<ListBucketResult><IsTruncated>false</IsTruncated><NextContinuationToken></NextContinuationToken></ListBucketResult>";
    let page = parse_list(xml).unwrap();
    assert!(page.objects.is_empty());
    assert_eq!(page.next_token, None);
}