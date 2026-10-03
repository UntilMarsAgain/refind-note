//! AWS SigV4：把一次请求算成一个 `Authorization` 头的值。
//!
//! 这个模块是整条链子里**唯一有标准答案**的一段 —— AWS 文档里公布了逐字节的测试向量，
//! 对上了就说明与别家实现互通（MinIO、R2、OSS、缤纷云认的都是这一套）。所以它的
//! 内部结构照着规范的四步走，一步不改：`sign` 里那三段注释就是规范的 1) 2) 3)。
//!
//! 另一半是**编码**（[`encode`] / [`encode_path`]）：SigV4 规定只放行
//! `A-Za-z0-9-_.~`，其余一律百分号编码，空格是 `%20`（不是 `+`）。路径要按段编、
//! 留着 `/` 分段，查询串则是整串编 —— 两者规则不同，所以是两个函数。

use hmac::{Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

/// 拼好的请求：地址、头、签名。发出去那一脚由调用方选（带不带正文）
pub(super) struct Prepared {
    pub(super) method: String,
    /// 用**配好的那个** agent 发（超时、4xx 不当错误都配在它身上）
    pub(super) agent: ureq::Agent,
    pub(super) url: String,
    pub(super) headers: Vec<(String, String)>,
    pub(super) authorization: String,
}

impl Prepared {
    /// 没有正文的那种（GET / HEAD / DELETE）
    pub(super) fn call(self) -> Result<ureq::http::Response<ureq::Body>, ureq::Error> {
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
    pub(super) fn send(self, body: &[u8]) -> Result<ureq::http::Response<ureq::Body>, ureq::Error> {
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
pub(super) struct SigningInput<'a> {
    pub(super) method: &'a str,
    /// 已经编码好的路径（含桶名）
    pub(super) path: &'a str,
    /// 已经编码并排好序的查询串
    pub(super) query: &'a str,
    /// 参与签名的头（小写名 + 值），至少含 `host`
    pub(super) headers: &'a [(String, String)],
    pub(super) payload_hash: &'a str,
    /// `20130524T000000Z`
    pub(super) stamp: &'a str,
    /// `20130524`
    pub(super) date: &'a str,
    pub(super) region: &'a str,
    pub(super) access_key: &'a str,
    pub(super) secret_key: &'a str,
}

/// AWS SigV4 的签名，返回 `Authorization` 头的值。
///
/// 单独拎出来是因为它是**唯一有标准答案**的一段：官方测试向量能逐字节对，
/// 对上了就说明与别家实现互通（MinIO、R2、OSS 认的都是这个）。
pub(super) fn sign(input: &SigningInput<'_>) -> String {
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

pub(super) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// 端点里的 host（含端口）——`Host` 头参与签名，所以要从地址里取出来
pub(super) fn host_of(endpoint: &str) -> String {
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
pub(super) fn encode_path(key: &str) -> String {
    key.split('/').map(encode).collect::<Vec<_>>().join("/")
}

/// SigV4 的百分号编码：只放行 `A-Za-z0-9-_.~`，空格写 `%20`
pub(super) fn encode(value: &str) -> String {
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