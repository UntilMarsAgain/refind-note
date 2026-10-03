//   Refind Note is a note-taking software.
//   Copyright (C) 2026 Until Mars Again
//
//   This program is free software: you can redistribute it and/or modify
//   it under the terms of the GNU Affero General Public License as published by
//   the Free Software Foundation, either version 3 of the License, or
//   (at your option) any later version.
//
//   This program is distributed in the hope that it will be useful,
//   but WITHOUT ANY WARRANTY; without even the implied warranty of
//   MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
//   GNU Affero General Public License for more details.
//
//   You should have received a copy of the GNU Affero General Public License
//   along with this program.  If not, see <http://www.gnu.org/licenses/>.

//! 一个**只够用**的 S3 客户端：取、放、看、删、列，加上条件写。
//!
//! ## 为什么自己写而不用官方 SDK
//!
//! 这个程序整条链子都是同步的，而 S3 的官方 SDK 会拽进一整套异步运行时与几十个 crate；
//! 真正要用的操作只有五个，签名算法（AWS SigV4）又是公开且固定的 —— 照着实现，再拿
//! 官方的测试向量对一遍，比引一整套 SDK 明白。底层就用 [`ureq`]，没有别的依赖。
//!
//! ## 面向哪些服务
//!
//! AWS S3 本身只是其中一种；这个客户端照的是 **S3 兼容** 那一档协议：MinIO、Ceph RGW、
//! Cloudflare R2、缤纷云这类自建或第三方服务都认同一套 SigV4 签名与 XML 清单，
//! 因此不引入任何厂商特有的东西。域名的两种写法（桶名在路径里 / 在域名里）也都认 ——
//! 判据见 [`S3::path_of`]。
//!
//! ## 分层
//!
//! 本文件只管**拼请求与判状态码**（`impl S3` 的那几个方法各对应一个操作），另外两段
//! 各自独立成文件：
//!
//! - [`signing`]：AWS SigV4 全部 —— 规范请求、待签串、派生钥匙、百分号编码；
//! - [`listing`]：把 S3 的响应读成结构体 —— 清单的 XML 与响应头（ETag 在两条路上
//!   写法不一样，得读成同一个值，见 [`listing::clean_etag`]）。
//!
//! ## 几处刻意的选择
//!
//! - **条件写**（`If-None-Match: *` / `If-Match: <etag>`）：云端的自旋锁就靠它 ——
//!   "没有这个对象时才写得进去"是 S3 原生的原子操作，不必自己发明；
//! - 4xx/5xx **不当错误**（`http_status_as_error(false)`）：404 是"没有这一份"、
//!   412 是"锁被人先占了"，都是有意义的结果，由调用方判断。

mod listing;
mod signing;

use std::sync::LazyLock;
use std::time::Duration;

use sha2::{Digest, Sha256};
use time::format_description::BorrowedFormatItem;
use time::OffsetDateTime;

#[cfg(test)]
mod tests;

use listing::{clean_etag, header, parse_list};
use signing::{Prepared, SigningInput, encode, encode_path, hex, host_of, sign};

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
    /// 服务地址。**两种写法都认**：
    ///
    /// - 不带桶名（`https://s3.example.com`）—— 桶名由程序补进路径；
    /// - 带桶名（`https://桶名.s3.example.com`）—— 有些服务（如缤纷云）给的端点
    ///   本来就长这样，这时程序不再补，否则桶名会被当成键的一部分：
    ///   上传看着成功，列举却永远列不到东西。
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
    /// URL 里那一段路径：`/桶名/键` 还是 `/键`。
    ///
    /// **两种写法服务端都可能有，认错了后果很重**：
    ///
    /// - 桶名在域名里（`https://桶名.s3.某云.net`，virtual-host）—— 服务端从域名取桶，
    ///   整条路径都当**键**。这时再补一段桶名，东西就存成 `桶名/db/…`：上传看着成功
    ///   （PUT 与 HEAD 走同一条路，自洽），可**列举**——列举也走这条路——多半列不到，
    ///   于是"云端一片空白"被读成"云端删过"，接着把本地的删掉。丢文件就是从这儿开始的。
    /// - 桶名在路径里（`https://s3.example.com/桶名/键`，path-style）—— 域名里没有桶，
    ///   路径必须补上那一段。
    ///
    /// 判据就一条：**域名的第一段是不是桶名**。是，就按域名里的桶来（不再补）；
    /// 不是（`127.0.0.1:9000`、`s3.example.com` 这种），才补。
    fn path_of(&self, key: &str) -> String {
        let bucket = self.config.bucket.trim();
        let first_label = host_of(&self.config.endpoint);
        let first_label = first_label.split('.').next().unwrap_or_default().to_string();

        if !bucket.is_empty() && first_label == bucket {
            format!("/{}", encode_path(key))
        } else {
            format!("/{}/{}", bucket, encode_path(key))
        }
    }

    fn prepare(
        &self,
        method: &str,
        key: &str,
        query: &str,
        extra: &[(&str, &str)],
        body: Option<&[u8]>,
    ) -> Prepared {
        let host = host_of(&self.config.endpoint);
        let path = self.path_of(key);
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