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

//! `refind://localhost/file/<名字>`：把文件页面的字节递给 webview。
//!
//! 只给正文里的 `<img>` / `<video>` / `<audio>` 用：笔记里写的是名字，渲染之后由前端
//! 换成这个地址。键照表查，查不到、文件不在、读不动都是 404 —— **不猜路径**。
//!
//! 认 `Range` 请求（`bytes=起点-终点`）：播放器拖进度条要的就是它，不然只能从头看。

use crate::open_database;
use crate::platform::decode_percent;
use crate::vault::namespace;

/// 取字节的地址头 —— 手拼这个地址的地方都得用它。
///
/// 自定义协议在各平台的写法**不一样**：
///
/// - **Windows 与 Android**：`http://refind.localhost/…`
///   （这两个 webview 不认自定义协议头，Tauri 把它们挂在 `.localhost` 上）
/// - Linux / macOS / iOS：`refind://localhost/…`
///
/// 这一条跟着 Tauri 自己的规则走（它内部那段 `convertFileSrc`）。拼错了，
/// 那两个平台上所有图片、音视频都取不到字节，而别的地方看着一切正常 ——
/// 前端同样的那条在 `dom/file-links.ts` 的 `FILE_ORIGIN`。
pub fn file_origin() -> &'static str {
    if cfg!(any(windows, target_os = "android")) {
        "http://refind.localhost"
    } else {
        "refind://localhost"
    }
}

/// `refind://localhost/file/<名字>` → 文件页面的字节。
///
/// 只给 webview 里的 `<img src>` 用：笔记正文里写的是相对名字，渲染之后由前端
/// 换成这个地址。键照表查，查不到、文件不在、读不动都是 404 —— **不猜路径**。
pub fn serve_file(request: &tauri::http::Request<Vec<u8>>) -> tauri::http::Response<Vec<u8>> {
    let missing = || {
        tauri::http::Response::builder()
            .status(404)
            .body(Vec::new())
            .expect("空响应总是拼得出来")
    };

    let Some(name) = request.uri().path().strip_prefix("/file/") else {
        return missing();
    };
    let Ok((_, database)) = open_database() else {
        return missing();
    };
    // `?rev=N` 取的是历史里的那一版；不写就是最新一版
    let rev = query_value(request.uri().query(), "rev");
    let title = format!("{}:{}", namespace::FILE_NAME, decode_percent(name));
    // 读不出来（没有这一页、上了锁、内容坏了）都是 404：这里只说"取不到"
    let Ok((bytes, mime)) = database.read_file(&title, rev.as_deref()) else {
        return missing();
    };

    let mime = if mime.is_empty() {
        "application/octet-stream".to_string()
    } else {
        mime
    };
    let total = bytes.len() as u64;

    // 播放器拖着进度条时会来要一段（`Range: bytes=起点-`）—— 认得出就给那一段，
    // 认不出就整份回去。视频能不能**拖动**，全看这一步
    let wanted = request
        .headers()
        .get("range")
        .and_then(|value| value.to_str().ok());
    if let Some((start, end)) = parse_range(wanted, total) {
        let slice = bytes[start as usize..=end as usize].to_vec();
        return tauri::http::Response::builder()
            .status(206)
            .header("Content-Type", mime)
            .header("Accept-Ranges", "bytes")
            .header("Content-Range", format!("bytes {start}-{end}/{total}"))
            .body(slice)
            .unwrap_or_else(|_| missing());
    }

    tauri::http::Response::builder()
        .header("Content-Type", mime)
        .header("Accept-Ranges", "bytes")
        .body(bytes)
        .unwrap_or_else(|_| missing())
}

/// `Range: bytes=起点-终点` → 闭区间 `(起点, 终点)`；认不出就是 `None`。
///
/// 只认单段、只认字节：多段的写法（`bytes=0-1,5-6`）没有播放器会用，
/// 认不了就整份回去，比猜错强。终点省略（`bytes=100-`）表示"到末尾"。
fn parse_range(header: Option<&str>, total: u64) -> Option<(u64, u64)> {
    let body = header?.trim().strip_prefix("bytes=")?.trim();
    if body.contains(',') {
        return None;
    }
    let (start, end) = body.split_once('-')?;
    let start: u64 = start.trim().parse().ok()?;
    let end: u64 = match end.trim() {
        "" => total.saturating_sub(1),
        value => value.parse().ok()?,
    };
    if start > end || end >= total {
        return None;
    }
    Some((start, end))
}

/// 查询串里的一个值（`rev=3` → `3`）；没有就是 `None`
fn query_value(query: Option<&str>, wanted: &str) -> Option<String> {
    query?
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(key, _)| *key == wanted)
        .map(|(_, value)| decode_percent(value))
        .filter(|value| !value.is_empty())
}

#[cfg(test)]
mod range_tests {
    use super::parse_range;

    #[test]
    fn ranges_are_read_only_when_they_make_sense() {
        // 常见三种：从头取、从中间取到末尾、取一段
        assert_eq!(parse_range(Some("bytes=0-"), 100), Some((0, 99)));
        assert_eq!(parse_range(Some("bytes=10-"), 100), Some((10, 99)));
        assert_eq!(parse_range(Some("bytes=10-20"), 100), Some((10, 20)));
        assert_eq!(parse_range(Some("bytes= 5 - 9 "), 100), Some((5, 9)));

        // 认不出的：别的单位、多段、越界、反着写、压根没给
        assert_eq!(parse_range(None, 100), None);
        assert_eq!(parse_range(Some("items=0-"), 100), None);
        assert_eq!(parse_range(Some("bytes=0-1,5-6"), 100), None);
        assert_eq!(parse_range(Some("bytes=0-100"), 100), None);
        assert_eq!(parse_range(Some("bytes=50-10"), 100), None);
        assert_eq!(parse_range(Some("bytes=abc-"), 100), None);
        // 空文件：没有可以给的段
        assert_eq!(parse_range(Some("bytes=0-"), 0), None);
    }
}
