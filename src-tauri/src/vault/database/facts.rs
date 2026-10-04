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

#[cfg(test)]
mod tests {
    use super::*;

    /// 数得对不对，是这一层存在的全部理由（数字给的是诊断页与设置页）。
    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "refind-note-facts-test-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(path: &std::path::Path, bytes: usize) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, vec![b'x'; bytes]).unwrap();
    }

    /// 空目录：零个文件、零字节 —— 而不是"数不出来"
    #[test]
    fn an_empty_directory_is_zero() {
        let dir = scratch("empty");
        assert_eq!(count_files(&dir), (0, 0));
        assert_eq!(directory_bytes(&dir), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **目录不存在**也是零而不是报错：诊断页在仓库还没建起来时也要有数
    #[test]
    fn a_missing_directory_is_zero_too() {
        let missing = std::env::temp_dir().join("refind-note-facts-test-does-not-exist");
        let _ = std::fs::remove_dir_all(&missing);
        assert_eq!(count_files(&missing), (0, 0));
    }

    /// 子目录里的文件也要数进去 —— blob 仓是分片的（`blobs/ab/<sha256>`）
    #[test]
    fn files_in_subdirectories_are_counted() {
        let dir = scratch("nested");
        write(&dir.join("titles.json"), 10);
        write(&dir.join("objects/0/abc.log"), 20);
        write(&dir.join("blobs/ab/cd"), 30);

        let (count, bytes) = count_files(&dir);
        assert_eq!(count, 3);
        assert_eq!(bytes, 60);
        assert_eq!(directory_bytes(&dir), 60);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 空目录本身不算一个文件 —— 仓库里到处是空目录（每个命名空间一个）
    #[test]
    fn an_empty_subdirectory_adds_nothing() {
        let dir = scratch("emptydir");
        write(&dir.join("a.json"), 5);
        std::fs::create_dir_all(dir.join("objects/0")).unwrap();

        assert_eq!(count_files(&dir), (1, 5));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 软链**不跟着进目录** —— 否则一个指向祖先的链能绕成死循环。
    ///
    /// 钉的是"**数**"而不是字节数：软链自己按 [`std::fs::DirEntry::metadata`]
    /// 算出来多长，取决于文件系统（它记的是目标路径那串字的长度），
    /// 而**跟不跟随**这件事在文件数上一望即知。
    #[cfg(unix)]
    #[test]
    fn a_symlinked_directory_is_not_followed() {
        let dir = scratch("link");
        write(&dir.join("real/data.json"), 7);
        write(&dir.join("real/inner/more.json"), 8);
        std::os::unix::fs::symlink(dir.join("real/inner"), dir.join("shortcut")).unwrap();

        // data.json + more.json + 软链本身 = 3
        // （跟进去的话 more.json 会被数两次，变成 4）
        assert_eq!(count_files(&dir).0, 3);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
