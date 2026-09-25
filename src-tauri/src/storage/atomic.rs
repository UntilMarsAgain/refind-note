//! 内容寻址的字节存储与原子写。
//!
//! 这一层刻意只有 S3 也需要的最小语义：按内容哈希存取不可变字节流、以及
//! 「写临时文件再改名」的原子替换。将来换成真正的 S3 时，这里对应 PutObject /
//! GetObject，去重则由 S3 或上层自行处理。

use sha2::{Digest, Sha256};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// SHA-256 十六进制（小写）
pub fn hash_bytes(bytes: &[u8]) -> String {
    // 一次性哈希够用；输出是 hybrid-array 的 Array<u8, U32>，没有 LowerHex，
    // 所以自己拼十六进制，也就不必再引 hex 之类的依赖。
    let digest = Sha256::digest(bytes);

    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest.iter() {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// 原子写：先写同目录下的临时文件，再改名覆盖。
/// 同目录改名在 POSIX 上是原子的，因此读方要么看到旧内容、要么看到新内容。
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        file.flush()?;
    }
    fs::rename(&tmp, path)
}

/// 追加一行（日志用）。只追加、不重写，崩溃最多丢最后一行。
pub fn append_line(path: &Path, line: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(file, "{line}")
}

/// 内容寻址的 blob 仓
#[derive(Debug, Clone)]
pub struct BlobStore {
    root: PathBuf,
}

impl BlobStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// 哈希分片路径：`<root>/ab/abcdef…`
    pub fn path_of(&self, hash: &str) -> PathBuf {
        let (prefix, _) = hash.split_at(hash.len().min(2));
        self.root.join(prefix).join(hash)
    }

    /// 写入并返回内容哈希；内容已存在则直接返回（天然去重）
    pub fn put(&self, bytes: &[u8]) -> io::Result<String> {
        let hash = hash_bytes(bytes);
        let path = self.path_of(&hash);
        if !path.is_file() {
            write_atomic(&path, bytes)?;
        }
        Ok(hash)
    }

    pub fn get(&self, hash: &str) -> io::Result<Vec<u8>> {
        fs::read(self.path_of(hash))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_is_stable_hex() {
        // 空串的 SHA-256 是众所周知的常量，拿来钉住实现
        assert_eq!(
            hash_bytes(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(hash_bytes(b"").len(), 64);
    }

    #[test]
    fn identical_content_is_stored_once() {
        let dir = std::env::temp_dir().join(format!("refind-store-{}", std::process::id()));
        let store = BlobStore::new(dir.clone());

        let first = store.put(b"hello").unwrap();
        let second = store.put(b"hello").unwrap();
        assert_eq!(first, second);

        // 目录里应当只有一个文件
        let mut count = 0;
        let prefix = dir.join(&first[..2]);
        for entry in std::fs::read_dir(&prefix).unwrap() {
            entry.unwrap();
            count += 1;
        }
        assert_eq!(count, 1);

        assert_eq!(store.get(&first).unwrap(), b"hello");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn append_line_creates_parent_dirs() {
        let dir = std::env::temp_dir().join(format!("refind-log-{}", std::process::id()));
        let path = dir.join("nested").join("a.log");
        append_line(&path, "one").unwrap();
        append_line(&path, "two").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "one\ntwo\n");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
