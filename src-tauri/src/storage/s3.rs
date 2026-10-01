//! 一个**只够用**的 S3 客户端：取、放、看、删、列，加上条件写。
//!
//! 为什么自己写而不用官方 SDK：这个程序整条链子都是同步的，而 S3 的官方 SDK 会
//! 拽进一整套异步运行时与几十个 crate；真正要用的操作只有五个，签名算法（AWS SigV4）
//! 又是公开且固定的 —— 照着实现，再拿官方的测试向量对一遍，比引一整套 SDK 明白。
//!
//! 几处刻意的选择：
//!
//! - **path-style**（`https://endpoint/bucket/key`）：MinIO、Ceph 这类自建服务多半这么用，
//!   而兼容它的服务也都认虚拟主机式之外这一种；
//! - **条件写**（`If-None-Match: *` / `If-Match: <etag>`）：云端的自旋锁就靠它 ——
//!   "没有这个对象时才写得进去"是 S3 原生的原子操作，不必自己发明；
//! - 4xx/5xx **不当错误**（`http_status_as_error(false)`）：404 是"没有这一份"、
//!   412 是"锁被人先占了"，都是有意义的结果，由调用方判断。

use std::sync::LazyLock;
use std::time::Duration;

use hmac::{Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256};
use time::format_description::BorrowedFormatItem;
use time::OffsetDateTime;

type HmacSha256 = Hmac<Sha256>;

/// 一次请求最多等多久（同步跑在启动路径上，不能没完没了地等）
const TIMEOUT: Duration = Duration::from_secs(30);

/// `20261001T120000Z` 这种格式：SigV4 要的写法
static STAMP_FORMAT: LazyLock<Vec<BorrowedFormatItem<'static>>> = LazyLock::new(|| {
    time::format_description::parse_borrowed::<2>("[year][month][day]T[hour][minute][second]Z")
        .expect("固定的格式串总是解析得出来")
});

/// 连哪一个服务、用哪把钥匙
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct S3Config {
    /// 服务地址，形如 `https://s3.example.com`（**不带**桶名）
    pub endpoint: String,
    /// 区域。多数兼容服务不校验，缺省 `us-east-1`
    pub region: String,
    pub bucket: String,
    /// 桶里的前缀（相当于"同步到哪一层目录"），可以为空
    pub prefix: String,
    pub access_key: String,
    pub secret_key: String,
}

impl S3Config {
    /// 该填的都填了吗（没填就当没开同步）
    pub fn is_usable(&self) -> bool {
        !self.endpoint.is_empty()
            && !self.bucket.is_empty()
            && !self.access_key.is_empty()
            && !self.secret_key.is_empty()
    }

    /// 桶里某个键的完整键名（前缀 + 相对路径）
    pub fn key_of(&self, relative: &str) -> String {
        let prefix = self.prefix.trim_matches('/');
        if prefix.is_empty() {
            relative.to_string()
        } else {
            format!("{prefix}/{relative}")
        }
    }

    fn region(&self) -> &str {
        if self.region.is_empty() {
            "us-east-1"
        } else {
            &self.region
        }
    }
}

/// 桶里的一个对象（`list` 与 `head` 的产物）
#[derive(Debug, Clone, PartialEq)]
pub struct Object {
    /// 完整键名
    pub key: String,
    pub size: u64,
    /// ETag（引号已去掉）。内容变了它就变 —— 用来判断"这一份是不是同一份"
    pub etag: String,
    /// 最后修改时间（RFC3339）
    pub modified: String,
}

/// 放在桶里的一份东西取回来的样子
pub struct Fetched {
    pub bytes: Vec<u8>,
    pub etag: String,
    pub modified: String,
}

/// 条件写没写成：写成了，还是"已经有人先在那儿了"
#[derive(Debug, PartialEq, Eq)]
pub enum PutOutcome {
    Written,
    /// `If-None-Match: *` 没通过：这个键已经存在（抢锁时就是这个意思）
    AlreadyExists,
    /// `If-Match: …` 没通过：内容已经被人换过了
    Changed,
}

pub struct S3 {
    config: S3Config,
    agent: ureq::Agent,
}

impl S3 {
    pub fn new(config: S3Config) -> Result<Self, String> {
        if !config.is_usable() {
            return Err("S3 配置不全：地址、桶、Access Key、Secret Key 都得填".to_string());
        }
        let agent = ureq::Agent::config_builder()
            // 4xx/5xx 自己判断：404 是"没有"、412 是"抢锁没抢到"，都不是异常
            .http_status_as_error(false)
            .timeout_global(Some(TIMEOUT))
            .build()
            .into();
        Ok(Self { config, agent })
    }

    pub fn config(&self) -> &S3Config {
        &self.config
    }

    /// 取一份。没有这一份返回 `None`（404 不是错误）
    pub fn get(&self, key: &str) -> Result<Option<Fetched>, String> {
        let response = self
            .prepare("GET", key, "", &[], None)
            .call()
            .map_err(|error| format!("请求失败：{error}"))?;
        match response.status().as_u16() {
            200 => {
                let etag = header(&response, "etag");
                let modified = header(&response, "last-modified");
                let bytes = response
                    .into_body()
                    .read_to_vec()
                    .map_err(|error| format!("读响应失败：{error}"))?;
                Ok(Some(Fetched {
                    bytes,
                    etag: clean_etag(&etag),
                    modified,
                }))
            }
            404 => Ok(None),
            status => Err(format!("取 {key} 失败：HTTP {status}")),
        }
    }

    /// 只看不看内容：大小、ETag、修改时间。没有这一份返回 `None`
    pub fn head(&self, key: &str) -> Result<Option<Object>, String> {
        let response = self
            .prepare("HEAD", key, "", &[], None)
            .call()
            .map_err(|error| format!("请求失败：{error}"))?;
        match response.status().as_u16() {
            200 => Ok(Some(Object {
                key: key.to_string(),
                size: header(&response, "content-length")
                    .parse()
                    .unwrap_or_default(),
                etag: clean_etag(&header(&response, "etag")),
                modified: header(&response, "last-modified"),
            })),
            404 => Ok(None),
            status => Err(format!("看 {key} 失败：HTTP {status}")),
        }
    }

    /// 放一份。`if_none_match` 为真时只在**这个键还不存在**时才写得进去（抢锁用）。
    pub fn put(&self, key: &str, bytes: &[u8], if_none_match: bool) -> Result<PutOutcome, String> {
        let conditions: Vec<(&str, &str)> = if if_none_match {
            vec![("if-none-match", "*")]
        } else {
            Vec::new()
        };
        let response = self
            .prepare("PUT", key, "", &conditions, Some(bytes))
            .send(bytes)
            .map_err(|error| format!("请求失败：{error}"))?;

        match response.status().as_u16() {
            200 => Ok(PutOutcome::Written),
            412 => Ok(PutOutcome::AlreadyExists),
            status => Err(format!("放 {key} 失败：HTTP {status}")),
        }
    }

    /// 换一份，但只在**内容还是我看到的那个**时才换（`If-Match: <etag>`）
    pub fn put_if(&self, key: &str, bytes: &[u8], etag: &str) -> Result<PutOutcome, String> {
        let conditions = vec![("if-match", etag)];
        let response = self
            .prepare("PUT", key, "", &conditions, Some(bytes))
            .send(bytes)
            .map_err(|error| format!("请求失败：{error}"))?;
        match response.status().as_u16() {
            200 => Ok(PutOutcome::Written),
            412 => Ok(PutOutcome::Changed),
            status => Err(format!("换 {key} 失败：HTTP {status}")),
        }
    }

    /// 删一份（删不存在的也算删掉了）
    pub fn delete(&self, key: &str) -> Result<(), String> {
        let response = self
            .prepare("DELETE", key, "", &[], None)
            .call()
            .map_err(|error| format!("请求失败：{error}"))?;
        match response.status().as_u16() {
            200 | 204 => Ok(()),
            404 => Ok(()),
            status => Err(format!("删 {key} 失败：HTTP {status}")),
        }
    }

    /// 列出前缀下的全部对象（自动翻页）
    pub fn list(&self, prefix: &str) -> Result<Vec<Object>, String> {
        let mut out = Vec::new();
        let mut token: Option<String> = None;

        loop {
            let mut query = format!(
                "continuation-token={}&list-type=2&prefix={}",
                encode(&token.clone().unwrap_or_default()),
                encode(prefix)
            );
            if token.is_none() {
                // 第一页不必带 continuation-token（空值会被有些服务当成"接着从空的地方"，干脆不写）
                query = format!("list-type=2&prefix={}", encode(prefix));
            }
            let response = self
                .prepare("GET", "", &query, &[], None)
                .call()
                .map_err(|error| format!("请求失败：{error}"))?;
            if response.status().as_u16() != 200 {
                return Err(format!(
                    "列 {prefix} 失败：HTTP {}",
                    response.status().as_u16()
                ));
            }
            let body = response
                .into_body()
                .read_to_vec()
                .map_err(|error| format!("读响应失败：{error}"))?;
            let page = parse_list(&String::from_utf8_lossy(&body))?;
            out.extend(page.objects);
            match page.next_token {
                Some(next) => token = Some(next),
                None => return Ok(out),
            }
        }
    }

    /// 拼一次请求：地址、该带的头、该签的名，一次算完
    ///
    /// `query` 传的是**已经编码并排好序**的形式（`a=1&b=2`），`key` 是桶里的键
    /// （空串表示操作桶本身，列清单就是这样）。
    fn prepare(
        &self,
        method: &str,
        key: &str,
        query: &str,
        extra: &[(&str, &str)],
        body: Option<&[u8]>,
    ) -> Prepared {
        let host = host_of(&self.config.endpoint);
        let path = format!("/{}/{}", self.config.bucket, encode_path(key));
        let url = format!(
            "{}{}?{}",
            self.config.endpoint.trim_end_matches('/'),
            path,
            query
        );
        let url = if query.is_empty() {
            url.trim_end_matches('?').to_string()
        } else {
            url
        };

        let payload_hash = hex(&Sha256::digest(body.unwrap_or(&[])));
        let now = OffsetDateTime::now_utc();
        let stamp = now.format(&STAMP_FORMAT).unwrap_or_default();
        let date = &stamp[..8];

        let mut headers: Vec<(String, String)> = vec![
            ("host".to_string(), host.clone()),
            ("x-amz-content-sha256".to_string(), payload_hash.clone()),
            ("x-amz-date".to_string(), stamp.clone()),
        ];
        for (name, value) in extra {
            headers.push((name.to_ascii_lowercase(), value.to_string()));
        }

        let authorization = sign(&SigningInput {
            method,
            path: &path,
            query,
            headers: &headers,
            payload_hash: &payload_hash,
            stamp: &stamp,
            date,
            region: self.config.region(),
            access_key: &self.config.access_key,
            secret_key: &self.config.secret_key,
        });

        Prepared {
            method: method.to_string(),
            agent: self.agent.clone(),
            url,
            // host 由 ureq 自己填，这里只带参与签名的其余头
            headers: headers
                .into_iter()
                .filter(|(name, _)| name != "host")
                .collect(),
            authorization,
        }
    }
}

/// 拼好的请求：地址、头、签名。发出去那一脚由调用方选（带不带正文）
struct Prepared {
    method: String,
    /// 用**配好的那个** agent 发（超时、4xx 不当错误都配在它身上）
    agent: ureq::Agent,
    url: String,
    headers: Vec<(String, String)>,
    authorization: String,
}

impl Prepared {
    /// 没有正文的那种（GET / HEAD / DELETE）
    fn call(self) -> Result<ureq::http::Response<ureq::Body>, ureq::Error> {
        let mut builder = match self.method.as_str() {
            "HEAD" => self.agent.head(&self.url),
            "DELETE" => self.agent.delete(&self.url),
            _ => self.agent.get(&self.url),
        };
        for (name, value) in &self.headers {
            builder = builder.header(name, value);
        }
        builder.header("authorization", self.authorization).call()
    }

    /// 带正文的那种（PUT）
    fn send(self, body: &[u8]) -> Result<ureq::http::Response<ureq::Body>, ureq::Error> {
        let mut builder = self.agent.put(&self.url);
        for (name, value) in &self.headers {
            builder = builder.header(name, value);
        }
        builder
            .header("authorization", self.authorization)
            .send(body)
    }
}

/// 参与签名的那些东西
struct SigningInput<'a> {
    method: &'a str,
    /// 已经编码好的路径（含桶名）
    path: &'a str,
    /// 已经编码并排好序的查询串
    query: &'a str,
    /// 参与签名的头（小写名 + 值），至少含 `host`
    headers: &'a [(String, String)],
    payload_hash: &'a str,
    /// `20130524T000000Z`
    stamp: &'a str,
    /// `20130524`
    date: &'a str,
    region: &'a str,
    access_key: &'a str,
    secret_key: &'a str,
}

/// AWS SigV4 的签名，返回 `Authorization` 头的值。
///
/// 单独拎出来是因为它是**唯一有标准答案**的一段：官方测试向量能逐字节对，
/// 对上了就说明与别家实现互通（MinIO、R2、OSS 认的都是这个）。
fn sign(input: &SigningInput<'_>) -> String {
    // 1) 规范请求
    let mut headers: Vec<&(String, String)> = input.headers.iter().collect();
    headers.sort_by(|a, b| a.0.cmp(&b.0));
    let canonical_headers: String = headers
        .iter()
        .map(|(name, value)| format!("{name}:{}\n", value.trim()))
        .collect();
    let signed_headers: Vec<&str> = headers.iter().map(|(name, _)| name.as_str()).collect();
    let signed_headers = signed_headers.join(";");

    let canonical_request = format!(
        "{}\n{}\n{}\n{}\n{}\n{}",
        input.method,
        input.path,
        input.query,
        canonical_headers,
        signed_headers,
        input.payload_hash
    );

    // 2) 待签串
    let scope = format!("{}/{}/s3/aws4_request", input.date, input.region);
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{}\n{}\n{}",
        input.stamp,
        scope,
        hex(&Sha256::digest(canonical_request.as_bytes()))
    );

    // 3) 派生钥匙：日期 → 区域 → 服务 → 收尾，一层层 HMAC
    let k_date = hmac(
        format!("AWS4{}", input.secret_key).as_bytes(),
        input.date.as_bytes(),
    );
    let k_region = hmac(&k_date, input.region.as_bytes());
    let k_service = hmac(&k_region, b"s3");
    let k_signing = hmac(&k_service, b"aws4_request");
    let signature = hex(&hmac(&k_signing, string_to_sign.as_bytes()));

    format!(
        "AWS4-HMAC-SHA256 Credential={}/{}, SignedHeaders={}, Signature={}",
        input.access_key, scope, signed_headers, signature
    )
}

fn hmac(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC 接受任意长度的钥匙");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// 端点里的 host（含端口）——`Host` 头参与签名，所以要从地址里取出来
fn host_of(endpoint: &str) -> String {
    let without_scheme = endpoint
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(endpoint);
    without_scheme
        .split('/')
        .next()
        .unwrap_or_default()
        .to_string()
}

/// 路径里的每一段做百分号编码（`/` 留着，其余按 SigV4 的规矩只放行 unreserved）
fn encode_path(key: &str) -> String {
    key.split('/').map(encode).collect::<Vec<_>>().join("/")
}

/// SigV4 的百分号编码：只放行 `A-Za-z0-9-_.~`，空格写 `%20`
fn encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn header(response: &ureq::http::Response<ureq::Body>, name: &str) -> String {
    response
        .headers()
        .get(name)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string()
}

/// ETag 两头带着引号，比的时候要去掉。
///
/// 去不干净是要出事的：云端那一份"变没变"全靠它比。清单走 XML（引号写成 `&quot;`
/// 或 `&#34;`）、GET/HEAD 走响应头（引号是字面的 `"`），两条路读出来的要是差一个
/// 字符，每趟都会判成"云端变了" —— 于是每次都把整份仓库重下一遍。
fn clean_etag(raw: &str) -> String {
    let mut text = raw.trim().to_string();
    loop {
        let before = text.clone();
        for quote in ["\"", "&quot;", "&#34;", "&#x22;"] {
            text = text
                .trim_start_matches(quote)
                .trim_end_matches(quote)
                .trim()
                .to_string();
        }
        if text == before {
            return text;
        }
    }
}

/// 一页清单
struct ListPage {
    objects: Vec<Object>,
    next_token: Option<String>,
}

/// 解析 `ListObjectsV2` 的 XML。
///
/// 只挑要用的三样（键、大小、ETag）与翻页标记：写成一个通用 XML 解析器不划算，
/// 而这份 XML 的形状是固定的。
fn parse_list(xml: &str) -> Result<ListPage, String> {
    let mut objects = Vec::new();
    for item in between(xml, "<Contents>", "</Contents>") {
        let key = tag(item, "Key")
            .map(|key| unescape_xml(&key))
            .unwrap_or_default();
        if key.is_empty() {
            continue;
        }
        objects.push(Object {
            key,
            size: tag(item, "Size")
                .and_then(|value| value.parse().ok())
                .unwrap_or_default(),
            etag: clean_etag(&unescape_xml(&tag(item, "ETag").unwrap_or_default())),
            modified: tag(item, "LastModified").unwrap_or_default(),
        });
    }

    // 只有被截断时才谈得上"下一页"
    let truncated = tag(xml, "IsTruncated")
        .map(|value| value.trim() == "true")
        .unwrap_or(false);
    let next_token = if truncated {
        tag(xml, "NextContinuationToken").map(|token| unescape_xml(&token))
    } else {
        None
    };

    Ok(ListPage {
        objects,
        next_token,
    })
}

/// 把 `xml` 里每一对 `<open>…</close>` 之间的内容抠出来
fn between<'a>(xml: &'a str, open: &str, close: &str) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find(open) {
        let after = &rest[start + open.len()..];
        let Some(end) = after.find(close) else {
            break;
        };
        out.push(&after[..end]);
        rest = &after[end + close.len()..];
    }
    out
}

/// 取一个 `<name>…</name>` 里的内容（不带命名空间前缀的写法）
fn tag(xml: &str, name: &str) -> Option<String> {
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    between(xml, &open, &close)
        .first()
        .map(|text| text.to_string())
}

/// XML 里那几个转义字符（键名里可能有 `&`、中文之类）
fn unescape_xml(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
