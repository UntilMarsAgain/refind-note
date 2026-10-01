//! 与**操作系统**打交道的那几件事。
//!
//! 这里的东西都离开不了"平台"两个字：协议在系统那边怎么注册、命令行参数怎么读、
//! `refind://` 的字节怎么递给 webview、要交给外部程序的文件先落到哪儿。
//! 它们与仓库本身没关系，所以从 `lib.rs` 里搬出来单放一处 —— `lib.rs` 只留装配。
//!
//! - [`deep_link`]：`refind://Help:首页` 的注册、解析与交付；
//! - [`protocol`]：`refind://localhost/file/<名字>` 的取字节服务（含 `Range`）；
//! - [`staging`]：交给系统应用打开时，先落到临时目录的那一步。

pub mod deep_link;
pub mod protocol;
pub mod staging;

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
