//! 字节仓：**地址 = 落盘字节的 sha256**。
//!
//! 为什么不拿明文哈希当地址：那等于给了人一个"验证某个猜测在不在这里"的工具。
//! 落盘字节的哈希在**没做任何变换时正好等于**明文哈希 —— 可那种情况下文件内容本来
//! 就明文可读，地址一点没多漏；一旦加了签名或加密，两者就分开了。
//!
//! 代价是签过、加过密的内容不再去重。这是认下的：gpg 签名带时间戳，本来就不该被
//! 当成同一份。**没做变换的仍然去重**，两条规则合起来是自洽的。
//!
//! 元数据（mime）也一样：它进头就进地址，同内容、不同 mime 会落成两个对象。

use std::fs;
use std::path::PathBuf;

use sha2::{Digest, Sha256};

use crate::codec::{self, Inspection, Meta, Policy, Protection, Secrets};
use crate::workspace::write_bytes;

/// SHA-256 十六进制（小写）
pub fn hash_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);

    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest.iter() {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

#[derive(Debug, Clone)]
pub struct BlobStore {
    root: PathBuf,
}

impl BlobStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// 哈希分片路径：`<root>/ab/abcdef…`。分片是为了别把几万个文件堆在一个目录里。
    pub fn path_of(&self, address: &str) -> PathBuf {
        let prefix = address.get(..2).unwrap_or(address);
        self.root.join(prefix).join(address)
    }

    pub fn has(&self, address: &str) -> bool {
        self.path_of(address).is_file()
    }

    /// 按策略落盘内容，返回地址。`meta` 是内容的自述（mime）—— 它进头、也进地址。
    pub fn put(
        &self,
        content: &[u8],
        meta: &Meta,
        policy: &Policy,
        passphrase: Option<&str>,
    ) -> Result<String, String> {
        let file =
            codec::encode(content, meta, policy, passphrase).map_err(|error| error.to_string())?;
        self.put_stored(&file)
    }

    /// 已经封装好的字节直接落盘（转存、GC 用得上）。
    pub fn put_stored(&self, file: &[u8]) -> Result<String, String> {
        let address = hash_hex(file);

        // 地址就是内容哈希，所以"文件已经在"就等于"内容一模一样"
        if !self.has(&address) {
            write_bytes(&self.path_of(&address), file)?;
        }

        Ok(address)
    }

    /// 取出落盘的原始字节（**没有解层**）
    pub fn read_stored(&self, address: &str) -> Result<Vec<u8>, String> {
        let path = self.path_of(address);
        fs::read(&path).map_err(|error| format!("读不出 {}：{error}", path.display()))
    }

    /// 取出内容（照头解层）
    pub fn get(&self, address: &str, secrets: &Secrets<'_>) -> Result<Vec<u8>, String> {
        let file = self.read_stored(address)?;
        codec::decode(&file, secrets).map_err(|error| error.to_string())
    }

    /// 只看头：保护状态与内容自述（**不需要口令**）
    pub fn inspect(&self, address: &str) -> Result<Inspection, String> {
        let file = self.read_stored(address)?;
        codec::inspect(&file).map_err(|error| error.to_string())
    }

    /// 只看头：这个 blob 是什么状态（**不需要口令**）
    pub fn protection(&self, address: &str) -> Result<Protection, String> {
        Ok(self.inspect(address)?.protection)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::Workspace;

    fn scratch(name: &str) -> (Workspace, BlobStore) {
        let dir = std::env::temp_dir().join(format!(
            "refind-note-store-test-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        let workspace = Workspace::open(dir).unwrap();
        let store = BlobStore::new(workspace.root().join("blobs"));
        (workspace, store)
    }

    #[test]
    fn plain_bytes_are_addressed_by_their_own_hash() {
        let (workspace, store) = scratch("plain");

        let address = store
            .put(b"hello", &Meta::default(), &Policy::default(), None)
            .unwrap();
        // 没做变换时，地址覆盖的是"头 + 明文"，所以仍能当完整性校验用
        let file = store.read_stored(&address).unwrap();
        assert_eq!(hash_hex(&file), address);
        assert_eq!(store.get(&address, &Secrets::default()).unwrap(), b"hello");

        let _ = fs::remove_dir_all(workspace.root());
    }

    #[test]
    fn the_same_content_twice_is_stored_once_when_nothing_varies() {
        let (workspace, store) = scratch("dedup");

        let first = store
            .put(b"same", &Meta::default(), &Policy::default(), None)
            .unwrap();
        let second = store
            .put(b"same", &Meta::default(), &Policy::default(), None)
            .unwrap();
        assert_eq!(first, second);
        assert!(store.path_of(&first).is_file());

        let _ = fs::remove_dir_all(workspace.root());
    }

    #[test]
    fn the_same_content_with_a_different_mime_is_a_different_blob() {
        let (workspace, store) = scratch("meta-address");

        // mime 进头就进地址：同内容、不同 mime 是两个对象（与"签过名的不去重"同一条规则）
        let markdown = Meta {
            mime: "text/markdown".to_string(),
        };
        let png = Meta {
            mime: "image/png".to_string(),
        };

        let first = store
            .put(b"same bytes", &markdown, &Policy::default(), None)
            .unwrap();
        let again = store
            .put(b"same bytes", &markdown, &Policy::default(), None)
            .unwrap();
        let other = store
            .put(b"same bytes", &png, &Policy::default(), None)
            .unwrap();

        assert_eq!(first, again, "同内容同 meta 去重");
        assert_ne!(first, other, "mime 不同就是另一个对象");

        // 头是明文：不用口令也知道它是什么
        assert_eq!(store.inspect(&other).unwrap().meta.mime, "image/png");

        let _ = fs::remove_dir_all(workspace.root());
    }

    #[test]
    fn encryption_takes_the_content_out_of_reach_and_keeps_a_plain_header() {
        let (workspace, store) = scratch("sealed");
        let policy = Policy {
            compress: true,
            symmetric: true,
            ..Default::default()
        };

        let address = store
            .put(b"secret words", &Meta::default(), &policy, Some("pw"))
            .unwrap();

        // 字节里翻不到原文
        let file = store.read_stored(&address).unwrap();
        assert!(!file.windows(12).any(|window| window == b"secret words"));

        // 头是明文：不输口令也知道这是加密的
        let protection = store.protection(&address).unwrap();
        assert!(protection.symmetric && protection.compress);

        // 口令对了才拿得到
        assert!(store.get(&address, &Secrets::default()).is_err());
        assert_eq!(
            store
                .get(
                    &address,
                    &Secrets {
                        passphrase: Some("pw")
                    }
                )
                .unwrap(),
            b"secret words"
        );

        let _ = fs::remove_dir_all(workspace.root());
    }

    #[test]
    fn a_broken_file_is_caught_by_the_address() {
        let (workspace, store) = scratch("broken");

        let address = store
            .put(b"hello", &Meta::default(), &Policy::default(), None)
            .unwrap();
        let path = store.path_of(&address);
        fs::write(&path, b"tampered").unwrap();

        // 地址对不上了 —— 这是白拿的一层完整性校验
        let file = store.read_stored(&address).unwrap();
        assert_ne!(hash_hex(&file), address);

        let _ = fs::remove_dir_all(workspace.root());
    }
}
