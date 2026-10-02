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

//! 与**操作系统**打交道的那几件事。
//!
//! 这里的东西都离开不了"平台"两个字：协议在系统那边怎么注册、命令行参数怎么读、
//! `refind://` 的字节怎么递给 webview、要交给外部程序的文件先落到哪儿。
//! 它们与仓库本身没关系，所以从 `lib.rs` 里搬出来单放一处 —— `lib.rs` 只留装配。
//!
//! - [`deep_link`]：`refind://Help:首页` 的注册、解析与交付；
//! - [`protocol`]：`refind://localhost/file/<名字>` 的取字节服务（含 `Range`）；
//! - [`saving`]：另存为 / 导出的东西落到哪个文件上（手机上统一进下载目录）；
//! - [`staging`]：交给系统应用打开时，先落到临时目录的那一步。

pub mod deep_link;
pub mod protocol;
pub mod saving;
pub mod staging;

/// 这台设备是什么：`"desktop"` 或 `"mobile"`（编译期就定了，不是运行时探测）。
///
/// 有几处要按它分叉：另存为走不走系统对话框（见 [`saving`]）、GPG 那一层在不在
/// （见 `storage::codec`）、仓库落在哪（见 `storage::workspace::install_root`）。
pub fn kind() -> &'static str {
    if cfg!(mobile) {
        "mobile"
    } else {
        "desktop"
    }
}

/// 把 URL 里的百分号编码换回原文（不是合法转义就原样留着）。
///
/// 三处都要用：二进制上传的文件名走请求头、`refind://` 的地址来自命令行、
/// 取字节的路径来自 URL —— 它们传过来的中文都是编码过的。
pub fn decode_percent(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
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

#[cfg(test)]
mod tests {
    use super::decode_percent;

    #[test]
    fn percent_escapes_come_back_as_text() {
        assert_eq!(decode_percent("%E6%A1%A5.png"), "桥.png");
        assert_eq!(decode_percent("桥.png"), "桥.png");
        // 不是合法转义就原样留着（名字里真有个 `%` 的也照旧）
        assert_eq!(decode_percent("100%"), "100%");
        assert_eq!(decode_percent("%ZZ"), "%ZZ");
    }
}
