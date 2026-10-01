//! 文件：附件也是**页面**，只不过正文是字节。
//!
//! 以前附件另立了一套（`files/` 目录 + 一张表），于是版本、加密、回收站、整理
//! 全都要再做一遍。现在它们就是 `File:` 命名空间里的页面（`File:桥.png`）：
//! 走的是与笔记**同一条路** —— 一版一个 blob、保护策略照旧、删除进回收站、
//! 整理按引用回收 —— 区别只在"读出来是字节而不是文本"。
//!
//! 于是"更新一个文件"就是**再提交一版**：历史留着，旧版随时能取回来。

use serde::Serialize;

use crate::storage::codec::{Inspection, Meta, Policy, Protection};
use crate::storage::session;
use crate::storage::workspace::write_bytes;
use crate::vault::database::Database;
use crate::vault::namespace::{FILE_ID, FILE_NAME};

/// 单个文件的体积上限（64 MiB：够放截图与文档，又不至于把内存撑爆）
pub const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;

/// 列表里的一项
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FileEntry {
    /// 显示标题（`File:桥.png`）
    pub title: String,
    /// 页面名（`桥.png`）—— 笔记里引用的就是它
    pub name: String,
    pub mime: String,
    pub size: u64,
    pub rev: u64,
    pub modified: String,
    /// 取字节的地址（`refind://…`）
    pub url: String,
    /// 这一版是怎么存的（**明文头里就有**，所以不解锁也报得出来）
    pub protection: Protection,
    /// 读它要不要先解锁（口令层或 gpg 加密层）—— 列表据此决定"是摆图还是摆锁"
    pub needs_unlock: bool,
}

/// 一个文件的现状：元信息 + 现在读不读得动
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FileInfo {
    #[serde(flatten)]
    pub entry: FileEntry,
    /// 口令正躺在本次会话里：为真时**不用再问**
    pub passphrase_ready: bool,
    /// 这一版需要口令才能读（对称层）
    pub needs_passphrase: bool,
    /// 这一版是 gpg 加密的（本机有没有私钥，真要读的时候才知道）
    pub needs_secret_key: bool,
}

/// 一次上传的结果
#[derive(Debug, Serialize)]
pub struct Uploaded {
    pub entry: FileEntry,
    /// 这一版是更新（之前已经有这一页）还是新建
    pub updated: bool,
}

impl Database {
    /// 全部文件，**新的在前**
    pub fn list_files(&self) -> Result<Vec<FileEntry>, String> {
        let titles = self.titles()?;
        let mut out = Vec::new();

        for (id, display) in &titles.notes {
            let Ok(parsed) = crate::vault::title::parse(display, &self.namespaces()) else {
                continue;
            };
            if parsed.ns != FILE_ID {
                continue;
            }
            let state = self.state_of(id)?;
            let inspection = self.inspect_rev(&state.blob)?;
            out.push(FileEntry {
                title: display.clone(),
                name: parsed.page.clone(),
                mime: inspection.meta.mime.clone(),
                size: state.bytes,
                rev: state.rev,
                modified: state.modified.clone(),
                url: url_of(&parsed.page, None),
                needs_unlock: Self::needs_unlock(&inspection.protection),
                protection: inspection.protection,
            });
        }

        out.sort_by(|a, b| b.modified.cmp(&a.modified));
        Ok(out)
    }

    /// 收一个文件：没有这一页就建一页，有就**加一版**（这就是"更新"）。
    ///
    /// `protection` 是这一版**怎么存**：不给就照这一页当前的（新建的页面就是仓库默认）——
    /// 文件进的是同一套 blob 仓，压缩、签名、加密、口令因此都成立，与笔记完全一样。
    pub fn add_file(
        &self,
        name: &str,
        bytes: &[u8],
        mime: &str,
        protection: Option<Policy>,
        passphrase: Option<String>,
    ) -> Result<Uploaded, String> {
        let name = clean_name(name)?;
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err(format!(
                "文件太大（{} 字节，上限 {MAX_FILE_BYTES} 字节）",
                bytes.len()
            ));
        }

        let display = format!("{FILE_NAME}:{name}");
        let updated = self.exists(&display);
        if !updated {
            self.create(&display)?;
        }

        // 保护策略照这一页当前的（新页面就是仓库默认）；显式给了就换，从这一版起粘住
        self.commit_bytes(
            &display,
            bytes,
            mime,
            Some("上传文件".to_string()),
            protection,
            passphrase,
        )?;

        let entry = self
            .list_files()?
            .into_iter()
            .find(|entry| entry.title == display)
            .ok_or_else(|| format!("刚写下的文件不见了：{display}"))?;
        Ok(Uploaded { entry, updated })
    }

    /// 改名：页面名变了，别的都不动（`File:旧名` → `File:新名`）
    pub fn rename_file(&self, title: &str, name: &str) -> Result<FileEntry, String> {
        let name = clean_name(name)?;
        let display = self.rename(title, &format!("{FILE_NAME}:{name}"))?;
        self.list_files()?
            .into_iter()
            .find(|entry| entry.title == display)
            .ok_or_else(|| format!("刚改名的文件不见了：{display}"))
    }

    /// 删一个文件：与删笔记是同一条路 —— 进回收站，随时捞得回来
    pub fn delete_file(&self, title: &str) -> Result<(), String> {
        self.delete(title)
    }

    /// 读一个文件的字节与 mime
    pub fn read_file(
        &self,
        title: &str,
        reference: Option<&str>,
    ) -> Result<(Vec<u8>, String), String> {
        self.read_bytes(title, reference)
    }

    /// 另存为：把某一版复制到用户选的位置
    pub fn export_file(&self, title: &str, target: &std::path::Path) -> Result<(), String> {
        let (bytes, _mime) = self.read_file(title, None)?;
        write_bytes(target, &bytes)
    }

    /// 这一版的头：mime + 存储方式。**不需要口令** —— 头本来就是明文。
    fn inspect_rev(&self, blob: &str) -> Result<Inspection, String> {
        if blob.is_empty() {
            return Ok(Inspection {
                protection: Protection {
                    compress: false,
                    compression: crate::storage::codec::Compression::default(),
                    sign: None,
                    encrypt: None,
                    symmetric: false,
                    cipher: crate::storage::codec::Cipher::default(),
                },
                meta: Meta::default(),
            });
        }
        self.blobs().inspect(blob)
    }

    /// 这一头的读法：要不要先解锁
    fn needs_unlock(protection: &Protection) -> bool {
        protection.symmetric || protection.encrypt.is_some()
    }

    /// 一个文件的现状（`key` 可以是页面名，也可以是 `File:名字` 这样的标题）。
    ///
    /// 它回答的是"这一版怎么存的、现在读不读得动" —— 于是界面可以在**不去读字节**的
    /// 前提下决定：直接显示，还是先摆一个"解锁"的按钮。
    ///
    /// `reference` 给版本 token 就看那一版（`@view-3` 里的 3）：历史里点进一版时用得着。
    pub fn file_info(&self, key: &str, reference: Option<&str>) -> Result<FileInfo, String> {
        let title = self.file_title(key);
        let id = self.locate(&title)?;
        let state = self.state_of(&id)?;
        let page = crate::vault::title::parse(&title, &self.namespaces())?.page;

        // 看的是哪一版：不给就是最新那版
        let rev = match reference {
            Some(token) => crate::vault::resolve::token_to_rev(token)?,
            None => state.rev,
        };
        let (blob, bytes, modified) = if rev == state.rev {
            (state.blob.clone(), state.bytes, state.modified.clone())
        } else {
            let (at, blob, bytes, _) = self.event_at(&id, &title, rev)?;
            (blob, bytes, at)
        };
        let inspection = self.inspect_rev(&blob)?;

        Ok(FileInfo {
            entry: FileEntry {
                title: title.clone(),
                name: page.clone(),
                mime: inspection.meta.mime.clone(),
                size: bytes,
                rev,
                modified,
                url: url_of(&page, Some(rev)),
                needs_unlock: Self::needs_unlock(&inspection.protection),
                protection: inspection.protection.clone(),
            },
            passphrase_ready: session::passphrase_for(&id, rev).is_some(),
            needs_passphrase: inspection.protection.symmetric,
            needs_secret_key: inspection.protection.encrypt.is_some(),
        })
    }

    /// `桥.png` 与 `File:桥.png` 都认：前者补上前缀，后者原样用
    pub fn file_title(&self, key: &str) -> String {
        let table = self.namespaces();
        match crate::vault::title::parse(key, &table) {
            Ok(parsed) if parsed.ns == FILE_ID => parsed.display(&table),
            _ => format!("{FILE_NAME}:{}", key.trim()),
        }
    }
}

/// 取字节的地址：`refind://localhost/file/<页面名>`（Rust 侧注册的协议）。
///
/// 给了 `rev` 就带上版本号 —— 看历史里的某一版时，取的是那一版的字节，
/// 与"最新一版"分得开（不然旧版的页面会显示成新版的样子）。
pub fn url_of(name: &str, rev: Option<u64>) -> String {
    let base = format!(
        "{}/file/{}",
        crate::platform::protocol::file_origin(),
        encode_key(name)
    );
    match rev {
        Some(rev) => format!("{base}?rev={rev}"),
        None => base,
    }
}

/// 地址里的键：只转义必要时的那几个字符，中文原样（便于排障时一眼看懂）
fn encode_key(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for byte in name.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// 去掉路径部分与首尾的点，剩下的就是名字
fn clean_name(name: &str) -> Result<String, String> {
    let tail = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(name)
        .trim()
        .trim_matches('.')
        .trim();
    let cleaned: String = tail.chars().filter(|ch| !ch.is_control()).collect();
    let cleaned = cleaned.trim().to_string();
    if cleaned.is_empty() {
        return Err("文件名不能为空".to_string());
    }
    Ok(cleaned)
}

/// 后缀 → MIME。认不出的一律当二进制流：宁可让浏览器不预览，也不要猜错类型。
pub fn mime_of(name: &str) -> &'static str {
    let extension = name
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_lowercase())
        .unwrap_or_default();

    match extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "avif" => "image/avif",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        "pdf" => "application/pdf",
        "txt" | "md" => "text/plain; charset=utf-8",
        "json" => "application/json",
        "zip" => "application/zip",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn scratch(name: &str) -> Database {
        let dir = std::env::temp_dir().join(format!(
            "refind-note-files-test-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        let workspace = crate::storage::workspace::Workspace::open(dir).unwrap();
        Database::open(&workspace).unwrap()
    }

    fn cleanup(database: &Database) {
        if let Some(root) = database.root().parent() {
            let _ = fs::remove_dir_all(root);
        }
    }

    #[test]
    fn a_file_is_a_page_in_the_file_namespace() {
        let database = scratch("upload");
        let uploaded = database
            .add_file("桥 图.png", b"PNG", "image/png", None, None)
            .unwrap();

        assert!(!uploaded.updated);
        assert_eq!(uploaded.entry.title, "File:桥 图.png");
        assert_eq!(uploaded.entry.name, "桥 图.png");
        assert_eq!(uploaded.entry.mime, "image/png");
        assert_eq!(uploaded.entry.size, 3);

        // 它是一篇**笔记**：查得到、有历史（一次提交）
        assert!(database.exists("File:桥 图.png"));
        assert_eq!(database.revisions_of("File:桥 图.png").unwrap().len(), 1);

        cleanup(&database);
    }

    #[test]
    fn uploading_again_keeps_the_old_version() {
        let database = scratch("update");
        database
            .add_file("图.png", "第一版".as_bytes(), "image/png", None, None)
            .unwrap();
        let again = database
            .add_file("图.png", "第二版更长".as_bytes(), "image/png", None, None)
            .unwrap();

        assert!(again.updated, "同一个名字再传一次是**更新**");
        assert_eq!(again.entry.rev, 2);

        // 旧版还在，取得到
        let (first, _) = database.read_file("File:图.png", Some("1")).unwrap();
        assert_eq!(first, "第一版".as_bytes());
        let (latest, _) = database.read_file("File:图.png", None).unwrap();
        assert_eq!(latest, "第二版更长".as_bytes());

        cleanup(&database);
    }

    #[test]
    fn deleting_a_file_goes_through_the_trash() {
        let database = scratch("delete");
        database
            .add_file("图.png", "字节".as_bytes(), "image/png", None, None)
            .unwrap();
        database.delete_file("File:图.png").unwrap();

        assert!(!database.exists("File:图.png"));
        assert!(database.list_files().unwrap().is_empty());
        // 回收站里躺着，能还原 —— 还原这条路同样"不读文本"（读它必失败）
        assert_eq!(database.list_trash().unwrap().len(), 1);
        database.restore("File:图.png").unwrap();
        assert!(database.exists("File:图.png"));
        let (bytes, _) = database.read_file("File:图.png", None).unwrap();
        assert_eq!(bytes, "字节".as_bytes());

        cleanup(&database);
    }

    #[test]
    fn renaming_moves_no_bytes_but_changes_the_name() {
        let database = scratch("rename");
        database
            .add_file("旧名.png", "字节".as_bytes(), "image/png", None, None)
            .unwrap();

        let renamed = database.rename_file("File:旧名.png", "新名.png").unwrap();
        assert_eq!(renamed.title, "File:新名.png");
        assert_eq!(renamed.rev, 1, "改名不是新的一版");

        let (bytes, _) = database.read_file("File:新名.png", None).unwrap();
        assert_eq!(bytes, "字节".as_bytes());
        assert!(!database.exists("File:旧名.png"));

        cleanup(&database);
    }

    #[test]
    fn the_file_namespace_is_where_they_live_and_refs_use_the_name() {
        let database = scratch("url");
        let uploaded = database
            .add_file("桥 图.png", "字节".as_bytes(), "image/png", None, None)
            .unwrap();
        assert_eq!(
            uploaded.entry.url,
            "refind://localhost/file/%E6%A1%A5%20%E5%9B%BE.png"
        );

        // 名字里的路径与首尾的点都去掉
        let uploaded = database
            .add_file(
                "/tmp/…/照片.jpg",
                "字节".as_bytes(),
                "image/jpeg",
                None,
                None,
            )
            .unwrap();
        assert_eq!(uploaded.entry.name, "照片.jpg");
        assert!(database
            .add_file(
                "...",
                "字节".as_bytes(),
                "application/octet-stream",
                None,
                None
            )
            .is_err());

        cleanup(&database);
    }

    /// 加密的文件：**不解锁也报得出"要口令"**（头是明文），而读它要口令
    #[test]
    fn an_encrypted_file_reports_that_it_needs_unlocking() {
        // 碰会话口令的用例要串行（锁是全局那一把）
        let _guard = crate::storage::session::test_lock::guard();
        let database = scratch("locked");
        // 让这一页以"口令加密"存
        let policy = crate::storage::codec::Policy {
            compress: true,
            symmetric: true,
            ..Default::default()
        };
        database.create("File:秘密.png").unwrap();
        database
            .commit_bytes(
                "File:秘密.png",
                "字节".as_bytes(),
                "image/png",
                None,
                Some(policy),
                Some("口令".to_string()),
            )
            .unwrap();

        // 刚用过口令：它还躺在本次会话里，所以"读得动"
        let info = database.file_info("秘密.png", None).unwrap();
        assert!(info.needs_passphrase, "头里写着有口令层");
        assert!(info.entry.needs_unlock);
        assert!(info.passphrase_ready, "刚用过的口令还在这次会话里");
        assert!(!info.needs_secret_key, "不是 gpg 那一层");

        // 名字与标题两种写法都认
        assert_eq!(
            database
                .file_info("File:秘密.png", None)
                .unwrap()
                .entry
                .name,
            "秘密.png"
        );

        // 忘掉之后：**报得出"要口令"，但读不出来** —— 界面据此摆那个解锁按钮
        crate::storage::session::forget_all();
        let info = database.file_info("秘密.png", None).unwrap();
        assert!(!info.passphrase_ready, "忘掉之后要重新问");
        assert!(database.read_file("File:秘密.png", None).is_err());

        crate::storage::session::unlock(
            &database.id_of("File:秘密.png").unwrap(),
            1,
            "口令".to_string(),
        );
        let (bytes, _mime) = database.read_file("File:秘密.png", None).unwrap();
        assert_eq!(bytes, "字节".as_bytes());

        crate::storage::session::forget_all();
        cleanup(&database);
    }

    /// 传一张**不是文本**的图：不该在写完之后再报一句"不是文本"
    /// （写入是写入、读回文本是另一回事 —— 上传路径不该顺手去读文本）
    #[test]
    fn uploading_binary_content_does_not_need_it_to_be_text() {
        let database = scratch("binary");
        // 0x89 打头的 PNG，按 UTF-8 解析必失败
        let png = [0x89u8, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        let uploaded = database
            .add_file("图.png", &png, "image/png", None, None)
            .unwrap();

        assert_eq!(uploaded.entry.rev, 1);
        assert_eq!(uploaded.entry.size, png.len() as u64);
        let (bytes, _) = database.read_file("File:图.png", None).unwrap();
        assert_eq!(bytes, png);

        cleanup(&database);
    }

    /// 上传时能指定"怎么存"：给了口令层，这一版就按它落盘（与笔记同一条规矩）
    #[test]
    fn a_file_can_be_stored_with_its_own_protection() {
        // 碰会话口令的用例要串行（锁是全局那一把）
        let _guard = crate::storage::session::test_lock::guard();
        let database = scratch("policy");
        let policy = crate::storage::codec::Policy {
            compress: true,
            symmetric: true,
            ..Default::default()
        };
        database
            .add_file(
                "秘密.txt",
                "字节".as_bytes(),
                "text/plain; charset=utf-8",
                Some(policy),
                Some("口令".to_string()),
            )
            .unwrap();

        // 头里写着有口令层，读得动是因为口令还在这次会话里
        let info = database.file_info("秘密.txt", None).unwrap();
        assert!(info.needs_passphrase, "上传时指定的口令层没落上");
        assert!(info.entry.needs_unlock);
        let (bytes, _) = database.read_file("File:秘密.txt", None).unwrap();
        assert_eq!(bytes, "字节".as_bytes());

        crate::storage::session::forget_all();
        assert!(database.read_file("File:秘密.txt", None).is_err());

        cleanup(&database);
    }

    #[test]
    fn oversize_files_are_refused_by_size() {
        let database = scratch("oversize");
        let error = database
            .add_file(
                "大.bin",
                &vec![0u8; (MAX_FILE_BYTES + 1) as usize],
                "application/octet-stream",
                None,
                None,
            )
            .unwrap_err();
        assert!(error.contains("文件太大"), "{error}");
        cleanup(&database);
    }

    #[test]
    fn the_extension_decides_the_mime() {
        assert_eq!(mime_of("a.png"), "image/png");
        assert_eq!(mime_of("a.JPEG"), "image/jpeg");
        assert_eq!(mime_of("不带后缀"), "application/octet-stream");
        assert_eq!(mime_of("a.unknownthing"), "application/octet-stream");
    }
}
