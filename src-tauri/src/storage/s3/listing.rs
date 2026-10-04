//! 把 S3 的响应读成结构体：清单的 XML 与响应头。
//!
//! 两条路各有一种坑，所以放在一起处理：
//!
//! - **清单**走 `ListObjectsV2` 的 XML。这份 XML 的形状是固定的（键、大小、ETag、
//!   最后修改时间，外加一个翻页标记），为它写个通用 XML 解析器不划算，所以手抠标签
//!   就够（[`between`] / [`tag`] / [`unescape_xml`]）。
//! - **响应头**里的 ETag 引号是字面的 `"`，XML 里的却是 `&quot;` / `&#34;`。
//!   同一条东西读出两个字符串是要出事的 —— 云端那一份"变没变"全靠 ETag 比，
//!   差一个字符，每趟都会判成"云端变了"，于是每次都把整份仓库重下一遍。
//!   [`clean_etag`] 是两条路的汇合点，两边都走它。

use super::Object;

pub(super) fn header(response: &ureq::http::Response<ureq::Body>, name: &str) -> String {
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
pub(super) fn clean_etag(raw: &str) -> String {
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
pub(super) struct ListPage {
    pub(super) objects: Vec<Object>,
    pub(super) next_token: Option<String>,
}

/// 解析 `ListObjectsV2` 的 XML。
///
/// 只挑要用的三样（键、大小、ETag）与翻页标记：写成一个通用 XML 解析器不划算，
/// 而这份 XML 的形状是固定的。
pub(super) fn parse_list(xml: &str) -> Result<ListPage, String> {
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
