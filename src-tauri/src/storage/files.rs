//! 附件：用户上传的图片与文档。
//!
//! 与笔记同一条思路：**磁盘上的名字是生成的 ASCII 标识**（`<id>.<ext>`），原始文件名放在
//! 表里（`files.json`）。于是中文名、空格、重名、跨系统的编码问题都不会变成文件名的一部分 ——
//! 备份、同步、搬到另一个系统时少一整类麻烦。
//!
//! 两条规则：
//!
//! - **内容相同就复用**：同一张图反复上传（换台机器、改个名）不会在磁盘上堆一堆；
//! - **重名的不同文件则改名**（`图片-1.png`），不覆盖、也不报错 —— 上传时最怕的就是
//!   一声不响把上一个文件顶掉。
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::api::FileEntry;
use super::error::VaultError;
use super::atomic::{hash_bytes, write_atomic};
use super::Vault;

/// 单个附件的大小上限。定这个数是为了给"手滑选了整个 ISO"一个明确的拒绝，而不是卡死界面。
pub const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;

/// `files.json` 的结构
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct FileTable {
    /// 按上传时间先后排列（界面要最新的在前，就自己倒过来）
    items: Vec<StoredFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredFile {
    id: String,
    name: String,
    extension: String,
    size: u64,
    sha256: String,
    uploaded: String,
    mime: String,
}

impl StoredFile {
    fn to_entry(&self) -> FileEntry {
        FileEntry {
            id: self.id.clone(),
            name: self.name.clone(),
            size: self.size,
            sha256: self.sha256.clone(),
            uploaded: self.uploaded.clone(),
            mime: self.mime.clone(),
            // 界面直接用这个地址取文件（见 lib.rs 的 `refind:` 方案）
            url: format!("refind://localhost/files/{}", self.id),
        }
    }

    /// 磁盘上的名字：生成的标识 + 扩展名（扩展名只为让取用时能判类型）
    fn stored_name(&self) -> String {
        if self.extension.is_empty() {
            self.id.clone()
        } else {
            format!("{}.{}", self.id, self.extension)
        }
    }
}

impl Vault {
    pub fn files_dir(&self) -> PathBuf {
        self.root.join("files")
    }

    fn files_table_path(&self) -> PathBuf {
        self.root.join("files.json")
    }

    fn read_files(&self) -> Result<FileTable, VaultError> {
        let path = self.files_table_path();
        match fs::read_to_string(&path) {
            Ok(text) => Ok(serde_json::from_str(&text)?),
            // 表还没建是**正常状态**（仓库里一个附件都没有），不是错误
            Err(_) => Ok(FileTable::default()),
        }
    }

    fn write_files(&self, table: &FileTable) -> Result<(), VaultError> {
        let text = serde_json::to_vec_pretty(table)?;
        // write_atomic 给的是 io::Error，这里换成仓库自己的错误类型
        Ok(write_atomic(&self.files_table_path(), &text)?)
    }

    /// 收进一份文件内容。`name` 是原始文件名（可以有中文、空格），`bytes` 是内容。
    pub fn add_file(&self, name: &str, bytes: &[u8]) -> Result<FileEntry, VaultError> {
        let clean = clean_name(name);
        if clean.is_empty() {
            return Err(VaultError::Corrupt("文件名不能为空".to_string()));
        }
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err(VaultError::Corrupt(format!(
                "文件太大（{} 字节，上限 {} 字节）",
                bytes.len(),
                MAX_FILE_BYTES
            )));
        }

        let sha256 = hash_bytes(bytes);
        let extension = extension_of(&clean);
        let mut table = self.read_files()?;

        // 内容一样就直接复用：名字也沿用已有的那一份，引用不会因此改变
        if let Some(existing) = table
            .items
            .iter()
            .find(|file| file.sha256 == sha256 && file.extension == extension)
        {
            return Ok(existing.to_entry());
        }

        let mime = mime_of(&extension);
        // 名字撞了就加后缀，绝不覆盖
        let unique = unique_name(&table, &clean);
        let id = file_id(&sha256, &unique);
        let stored = StoredFile {
            id,
            name: unique,
            extension,
            size: bytes.len() as u64,
            sha256,
            uploaded: super::now_iso(),
            mime,
        };

        fs::create_dir_all(self.files_dir())?;
        fs::write(self.files_dir().join(stored.stored_name()), bytes)?;
        let entry = stored.to_entry();
        table.items.push(stored);
        self.write_files(&table)?;
        Ok(entry)
    }

    /// 重命名一个附件：**只改表里的那一行**。
    ///
    /// 磁盘上的名字是生成的标识，所以改名一个文件都不用动 —— 这正是当初把"磁盘名"与
    /// "显示名"分开的理由之一。
    ///
    /// 内容类型（`extension` / `mime`）**不跟着名字走**：它们描述的是内容本身，
    /// 把 `图.png` 改成 `照片.jpg` 并不会让里面的字节变成 JPEG。
    ///
    /// 重名时**报错**而不是自动加后缀：上传时加后缀是体贴（用户只想把文件放进来），
    /// 改名是刻意动作 —— 这时"这个名字有人用了"才是他需要知道的事。
    pub fn rename_file(&self, id: &str, name: &str) -> Result<FileEntry, VaultError> {
        let clean = clean_name(name);
        if clean.is_empty() {
            return Err(VaultError::Corrupt("文件名不能为空".to_string()));
        }
        let mut table = self.read_files()?;
        let index = table
            .items
            .iter()
            .position(|file| file.id == id)
            .ok_or_else(|| VaultError::NotFound(format!("附件 {id}")))?;

        let taken = table
            .items
            .iter()
            .enumerate()
            .any(|(other, file)| other != index && file.name == clean);
        if taken {
            return Err(VaultError::NameTaken(clean));
        }

        table.items[index].name = clean;
        let entry = table.items[index].to_entry();
        self.write_files(&table)?;
        Ok(entry)
    }

    /// 全部附件，最新的在前
    pub fn list_files(&self) -> Result<Vec<FileEntry>, VaultError> {
        let mut table = self.read_files()?;
        table.items.reverse();
        Ok(table.items.iter().map(StoredFile::to_entry).collect())
    }

    /// 取文件时的映射：`key` 可以是**标识**，也可以是**显示名**。
    ///
    /// 两种都在表里查 —— 请求里的那串字永远不会变成文件系统路径，所以路径穿越在
    /// 这一层就不成立。笔记里写的是名字（人记得住），界面取件时用的是标识，
    /// 两条路都从这里过。
    pub fn file_path_by_key(&self, key: &str) -> Result<Option<PathBuf>, VaultError> {
        let wanted = decode_key(key);
        if wanted.is_empty() {
            return Ok(None);
        }
        Ok(self
            .read_files()?
            .items
            .iter()
            .find(|file| file.id == wanted || file.name == wanted)
            .map(|file| self.files_dir().join(file.stored_name())))
    }

    /// 删除一个附件（连同磁盘上的那份内容）
    pub fn delete_file(&self, id: &str) -> Result<(), VaultError> {
        let mut table = self.read_files()?;
        let Some(index) = table.items.iter().position(|file| file.id == id) else {
            return Err(VaultError::NotFound(format!("附件 {id}")));
        };
        let removed = table.items.remove(index);
        // 先写表再删文件：万一半路失败，留下的是一个没人引用的文件（可回收），
        // 而不是"表里写着、磁盘上却没有"的空条目
        self.write_files(&table)?;
        let path = self.files_dir().join(removed.stored_name());
        if path.exists() {
            fs::remove_file(path)?;
        }
        Ok(())
    }
}

/// 解 URL 里的百分号编码：`%E5%9B%BE.png` → `图.png`。
///
/// 自己写十几行而不是引一个依赖：只用得上解码，而且这样能被用例钉住
/// （编码那一侧由前端的 `encodeURIComponent` 负责，两边对上就行）。
pub fn decode_key(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or("");
            if let Ok(value) = u8::from_str_radix(hex, 16) {
                out.push(value);
                index += 3;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

/// 只取文件名部分，并去掉一定不能出现在文件名里的字符（路径分隔符与控制字符）。
///
/// 不做"过滤成 ASCII"这种事：中文名是允许的，因为磁盘上用的不是它。
fn clean_name(name: &str) -> String {
    let tail = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(name)
        .trim()
        .trim_matches('.');
    tail.chars()
        .filter(|ch| !ch.is_control())
        .collect::<String>()
        .trim()
        .to_string()
}

/// 扩展名：只留小写 ASCII 字母数字，最多 8 位；认不出来就空着
fn extension_of(name: &str) -> String {
    let Some((_, extension)) = name.rsplit_once('.') else {
        return String::new();
    };
    let lowered = extension.to_lowercase();
    if lowered.is_empty() || lowered.len() > 8 || !lowered.chars().all(|ch| ch.is_ascii_alphanumeric())
    {
        return String::new();
    }
    lowered
}

/// 重名时在扩展名前加序号：`图片.png` → `图片-1.png`
fn unique_name(table: &FileTable, wanted: &str) -> String {
    if !table.items.iter().any(|file| file.name == wanted) {
        return wanted.to_string();
    }
    let (stem, extension) = match wanted.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() => (stem, format!(".{extension}")),
        _ => (wanted, String::new()),
    };
    for n in 1..10_000 {
        let candidate = format!("{stem}-{n}{extension}");
        if !table.items.iter().any(|file| file.name == candidate) {
            return candidate;
        }
    }
    format!("{stem}-{}", super::now_iso())
}

/// 扩展名 → 取文件时的类型。认不出来就给二进制流，让浏览器自己嗅探。
pub fn mime_of(extension: &str) -> String {
    match extension {
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
    .to_string()
}

/// 附件标识：内容 + 名字 + 时间的摘要取前 16 位。
///
/// 与笔记的标识同一个思路（够短、够散、不重复），但**不共用**同一个函数：
/// 两者的输入与用途不同，硬凑成一个反而要看调用方猜。
fn file_id(sha256: &str, name: &str) -> String {
    hash_bytes(format!("{sha256}:{name}:{}", super::now_iso()).as_bytes())
        .chars()
        .take(16)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::tests::TempVault;

    #[test]
    fn add_list_find_and_delete() {
        let temp = TempVault::new();
        let vault = &temp.vault;

        assert!(vault.list_files().unwrap().is_empty());

        let entry = vault.add_file("图片 一.png", b"hello").unwrap();
        assert_eq!(entry.name, "图片 一.png");
        assert_eq!(entry.size, 5);
        assert_eq!(entry.mime, "image/png");
        assert!(entry.url.ends_with(&entry.id));
        // 磁盘上的名字是生成的标识，不含原始文件名
        assert!(vault.files_dir().join(format!("{}.png", entry.id)).exists());

    }

    #[test]
    fn same_content_is_stored_once() {
        let temp = TempVault::new();
        let first = temp.vault.add_file("a.png", b"same").unwrap();
        let again = temp.vault.add_file("另一个名字.png", b"same").unwrap();
        // 复用已有条目：名字也随之保持第一次的那个，引用不会突然指向别处
        assert_eq!(again.id, first.id);
        assert_eq!(again.name, "a.png");
        assert_eq!(temp.vault.list_files().unwrap().len(), 1);
    }

    #[test]
    fn different_content_with_same_name_gets_a_suffix() {
        let temp = TempVault::new();
        let first = temp.vault.add_file("图片.png", b"one").unwrap();
        let second = temp.vault.add_file("图片.png", b"two").unwrap();
        assert_eq!(first.name, "图片.png");
        assert_eq!(second.name, "图片-1.png");
        assert_ne!(first.id, second.id);
        assert_eq!(temp.vault.list_files().unwrap().len(), 2);
    }

    #[test]
    fn names_are_reduced_to_a_basename() {
        let temp = TempVault::new();
        // 路径与多余的点都不该出现在名字里
        let entry = temp.vault.add_file("../../etc/图片.png", b"x").unwrap();
        assert_eq!(entry.name, "图片.png");
        let entry = temp.vault.add_file(".隐藏", b"y").unwrap();
        assert_eq!(entry.name, "隐藏");
        // 没有扩展名也给存，扩展名为空
        let entry = temp.vault.add_file("说明", b"z").unwrap();
        assert!(entry.url.starts_with("refind://localhost/files/"));
        assert_eq!(entry.mime, "application/octet-stream");
    }

    #[test]
    fn rename_only_touches_the_table() {
        let temp = TempVault::new();
        let entry = temp.vault.add_file("旧名.png", b"data").unwrap();
        let stored = temp.vault.files_dir().join(format!("{}.png", entry.id));
        assert!(stored.exists());

        let renamed = temp.vault.rename_file(&entry.id, "新名.png").unwrap();
        assert_eq!(renamed.name, "新名.png");
        // 标识与磁盘上的那份都没动 —— 改名只是改表
        assert_eq!(renamed.id, entry.id);
        assert!(stored.exists(), "内容不该被搬动");
        assert_eq!(temp.vault.list_files().unwrap()[0].name, "新名.png");

        // 名字里的路径部分照样只取文件名
        let cleaned = temp.vault.rename_file(&entry.id, "../别处/第三个名字.png").unwrap();
        assert_eq!(cleaned.name, "第三个名字.png");
    }

    #[test]
    fn rename_reports_a_taken_name_instead_of_suffixing() {
        let temp = TempVault::new();
        let first = temp.vault.add_file("甲.png", b"one").unwrap();
        let second = temp.vault.add_file("乙.png", b"two").unwrap();

        let message = temp
            .vault
            .rename_file(&second.id, "甲.png")
            .err()
            .expect("重名应当报错")
            .to_string();
        assert!(message.contains("甲.png"), "{message}");
        assert!(message.contains("换一个名字"), "{message}");

        // 改成自己原来的名字不算冲突
        assert!(temp.vault.rename_file(&second.id, "乙.png").is_ok());
        // 空名字要拒绝
        assert!(temp.vault.rename_file(&first.id, "   ").is_err());
        // 不存在的标识要报"找不到"，不是静静成功
        assert!(temp.vault.rename_file("deadbeef", "随便.png").is_err());
    }

    #[test]
    fn rename_keeps_the_content_type() {
        let temp = TempVault::new();
        let entry = temp.vault.add_file("图.png", b"\x89PNG").unwrap();
        let renamed = temp.vault.rename_file(&entry.id, "照片.jpg").unwrap();
        // 内容没有变，所以类型与磁盘上的扩展名都不该跟着名字走
        assert_eq!(renamed.mime, "image/png");
        assert!(temp
            .vault
            .files_dir()
            .join(format!("{}.png", entry.id))
            .exists());
    }

    #[test]
    fn delete_removes_both_the_entry_and_the_content() {
        let temp = TempVault::new();
        let entry = temp.vault.add_file("图.png", b"data").unwrap();
        let path = temp.vault.files_dir().join(format!("{}.png", entry.id));
        assert!(path.exists());

        temp.vault.delete_file(&entry.id).unwrap();
        assert!(temp.vault.list_files().unwrap().is_empty());
        assert!(!path.exists(), "内容也该一起删掉");

        // 删不存在的：给明确的错，不是静静成功
        assert!(temp.vault.delete_file(&entry.id).is_err());
    }

    #[test]
    fn keys_are_looked_up_in_the_table_only() {
        let temp = TempVault::new();
        let entry = temp.vault.add_file("桥 图.png", b"data").unwrap();

        // 标识与显示名都能取到同一份文件
        let by_id = temp.vault.file_path_by_key(&entry.id).unwrap().unwrap();
        let by_name = temp.vault.file_path_by_key("桥 图.png").unwrap().unwrap();
        assert_eq!(by_id, by_name);

        // 前端会用 encodeURIComponent，这里对上：解码后仍是同一个名字
        let encoded = "%E6%A1%A5%20%E5%9B%BE.png";
        assert_eq!(decode_key(encoded), "桥 图.png");
        assert!(temp.vault.file_path_by_key(encoded).unwrap().is_some());

        // 路径花招一律不成立：表里没有这个名字，就没有这个文件
        for bad in ["../vault.json", "..", "", "..%2Fvault.json", "桥 图.png/../vault.json"] {
            assert!(
                temp.vault.file_path_by_key(bad).unwrap().is_none(),
                "{bad} 不该取到文件"
            );
        }
    }

    #[test]
    fn percent_decoding_is_forgiving() {
        // 不是合法编码的地方原样保留，不吞字符
        assert_eq!(decode_key("abc"), "abc");
        assert_eq!(decode_key("100%"), "100%");
        assert_eq!(decode_key("%E5%9B%BE"), "图");
        assert_eq!(decode_key("%ZZ"), "%ZZ");
        assert_eq!(decode_key(""), "");
    }

    #[test]
    fn oversized_files_are_refused_with_a_readable_message() {
        let temp = TempVault::new();
        let huge = vec![0u8; (MAX_FILE_BYTES + 1) as usize];
        let message = temp
            .vault
            .add_file("大.bin", &huge)
            .err()
            .expect("超限应当被拒绝")
            .to_string();
        assert!(message.contains("太大"), "{message}");
    }
}
