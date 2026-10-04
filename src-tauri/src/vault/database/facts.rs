//! 仓库里有多少东西：日志几份、草稿几个、占多少字节。
//!
//! 只给**诊断页与设置页**看的纯统计：它读目录、不写任何东西，也就不怕被谁并发改动
//! 搞出半截结果 —— 数字是给人看个大概，不是账。

use serde::Serialize;

/// 仓库里有多少东西（诊断页）
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RepositoryFacts {
    /// 事件日志几份（`db/objects/<命名空间>/<id>.log`）
    pub logs: usize,
    /// 草稿槽位几个（`db/drafts/`，写了一半的）
    pub drafts: usize,
    /// 回收站里几条（`trash/`）
    pub trash: usize,
    /// 内容块几个（`db/blobs/ab/<sha256>`，内容寻址）
    pub blobs: usize,
    /// 内容块一共占多少字节（磁盘上那份，含压缩/加密后的封装）
    pub blob_bytes: u64,
    /// `db/` 整个目录占多少字节
    pub database_bytes: u64,
}

/// 数一数目录里有多少个文件、一共多少字节（子目录也算；数不出来就是 0）
pub(super) fn count_files(directory: &std::path::Path) -> (usize, u64) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return (0, 0);
    };

    let mut count = 0usize;
    let mut bytes = 0u64;
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            let (inner, inner_bytes) = count_files(&entry.path());
            count += inner;
            bytes += inner_bytes;
            continue;
        }
        count += 1;
        bytes += entry.metadata().map(|meta| meta.len()).unwrap_or(0);
    }
    (count, bytes)
}

/// 整个目录占多少字节（数不出来就是 0）
pub(super) fn directory_bytes(directory: &std::path::Path) -> u64 {
    count_files(directory).1
}
