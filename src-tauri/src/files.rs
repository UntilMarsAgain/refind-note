//! 附件：上传、改名、列出、删除。
//!
//! 一条笔记引用一个附件，写的是**名字**（`![桥](桥.png)`），而磁盘上存的是**标识** ——
//! 这样改名不会牵动任何一篇笔记里的字，名字里也能有中文、空格、大写。
//!
//! ```text
//! db/
//!   files/<标识>[.<后缀>]   附件的字节
//!   files.json              { "items": [ { id, name, extension, size, sha256, uploaded, mime } ] }
//! ```
//!
//! 表是**唯一的真相**：从键（标识或名字）到路径只有一条路 —— 查表。
//! 所以 `../` 这类东西没有出场的机会。
//!
//! 附件**不进内容块仓库**：那是笔记正文的地盘，它按内容寻址、会在整理时被回收；
//! 附件的字节是"用户自己放进去的东西"，不该有任何一轮整理去动它。

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::database::{now, Database};
use crate::store::hash_hex;
use crate::workspace::write_bytes;

/// 单个附件的体积上限（64 MiB：够放截图与文档，又不至于把内存撑爆）
pub const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;

/// 表里的一个附件
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileEntry {
    /// 标识：文件名与地址都用它
    pub id: String,
    /// 显示名（笔记里引用的就是它）
    pub name: String,
    /// 后缀（没有就是空串）；只影响 `mime`，改名不动它
    #[serde(default)]
    pub extension: String,
    pub size: u64,
    pub sha256: String,
    pub uploaded: String,
    pub mime: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FileTable {
    #[serde(default)]
    pub items: Vec<FileEntry>,
}

/// 一次上传的结果：新收的、还是原本就有一份一样的
#[derive(Debug, Serialize)]
pub struct Uploaded {
    pub entry: FileEntry,
    /// 内容与后缀都一样，直接复用了已有的那一份
    pub reused: bool,
}

impl Database {
    pub fn files_dir(&self) -> PathBuf {
        self.root().join("files")
    }

    fn files_table_path(&self) -> PathBuf {
        self.root().join("files.json")
    }

    pub fn files(&self) -> FileTable {
        crate::workspace::read_json(&self.files_table_path())
    }

    fn save_files(&self, table: &FileTable) -> Result<(), String> {
        crate::workspace::write_json(&self.files_table_path(), table)
    }

    /// 全部附件，**新的在前**
    pub fn list_files(&self) -> Result<Vec<FileEntry>, String> {
        let mut items = self.files().items;
        items.reverse();
        Ok(items)
    }

    /// 收一个附件。
    ///
    /// 内容与后缀都一样就直接复用已有那一份（同一个标识、同一个名字）——
    /// 重复粘贴同一张图不该在仓库里留下第二份。
    pub fn add_file(&self, name: &str, bytes: &[u8]) -> Result<Uploaded, String> {
        let name = clean_name(name)?;
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err(format!(
                "文件太大（{} 字节，上限 {MAX_FILE_BYTES} 字节）",
                bytes.len()
            ));
        }

        let sha256 = hash_hex(bytes);
        let extension = extension_of(&name);

        let mut table = self.files();
        if let Some(found) = table
            .items
            .iter()
            .find(|item| item.sha256 == sha256 && item.extension == extension)
        {
            return Ok(Uploaded {
                entry: found.clone(),
                reused: true,
            });
        }

        let id = file_id(&sha256, &name);
        let entry = FileEntry {
            id: id.clone(),
            name: unique_name(&table, &name),
            extension: extension.clone(),
            size: bytes.len() as u64,
            sha256,
            uploaded: now(),
            mime: mime_of(&extension).to_string(),
        };

        fs::create_dir_all(self.files_dir()).map_err(|error| format!("建不出附件目录：{error}"))?;
        write_bytes(&self.files_dir().join(disk_name(&id, &extension)), bytes)?;

        table.items.push(entry.clone());
        self.save_files(&table)?;
        Ok(Uploaded {
            entry,
            reused: false,
        })
    }

    /// 改名。**只改表**：磁盘上的文件名是标识，笔记里写的是这个名字 ——
    /// 两处都不会因为改名而动。
    pub fn rename_file(&self, id: &str, name: &str) -> Result<FileEntry, String> {
        let name = clean_name(name)?;
        let mut table = self.files();

        if table
            .items
            .iter()
            .any(|item| item.id != id && item.name == name)
        {
            return Err(format!("「{name}」已经存在，换一个名字"));
        }

        let Some(item) = table.items.iter_mut().find(|item| item.id == id) else {
            return Err(format!("没有这个附件：{id}"));
        };
        item.name = name;
        let updated = item.clone();
        self.save_files(&table)?;
        Ok(updated)
    }

    /// 删一个附件：先改表，再删文件。
    ///
    /// 顺序不能反 —— 反过来的话，中间崩了会留下"表里还记着、文件已经没了"的死条目。
    /// 这一版反着留的是"文件还在、没人认识"，那是整理那一轮的事。
    pub fn delete_file(&self, id: &str) -> Result<(), String> {
        let mut table = self.files();
        let Some(index) = table.items.iter().position(|item| item.id == id) else {
            return Err(format!("没有这个附件：{id}"));
        };

        let entry = table.items.remove(index);
        self.save_files(&table)?;

        let path = self
            .files_dir()
            .join(disk_name(&entry.id, &entry.extension));
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!("删不掉 {}：{error}", path.display())),
        }
    }

    /// 键（标识或名字）→ 表里的那一条。**认不出就是认不出**，不去猜路径。
    pub fn find_file(&self, key: &str) -> Option<FileEntry> {
        let key = decode_key(key);
        if key.is_empty() {
            return None;
        }
        self.files()
            .items
            .into_iter()
            .find(|item| item.id == key || item.name == key)
    }

    /// 键 → 磁盘上的路径
    pub fn file_path(&self, key: &str) -> Option<PathBuf> {
        let entry = self.find_file(key)?;
        let path = self
            .files_dir()
            .join(disk_name(&entry.id, &entry.extension));
        path.is_file().then_some(path)
    }

    /// 读一个附件的内容（另存为用）
    pub fn read_file(&self, key: &str) -> Result<Vec<u8>, String> {
        let path = self
            .file_path(key)
            .ok_or_else(|| format!("没有这个附件：{key}"))?;
        fs::read(&path).map_err(|error| format!("读不出 {}：{error}", path.display()))
    }

    /// 附件表里那些**文件已经不在**的条目（诊断页用得上）
    pub fn missing_files(&self) -> Vec<FileEntry> {
        self.files()
            .items
            .into_iter()
            .filter(|item| {
                !self
                    .files_dir()
                    .join(disk_name(&item.id, &item.extension))
                    .is_file()
            })
            .collect()
    }
}

/// 磁盘上的文件名：标识 + 后缀。后缀只是为了 `serve` 时报得准 mime
fn disk_name(id: &str, extension: &str) -> String {
    if extension.is_empty() {
        id.to_string()
    } else {
        format!("{id}.{extension}")
    }
}

/// 标识：内容哈希 + 名字 + 时间一起哈希，取前 16 位。
///
/// 为什么不直接用内容哈希：那样两个**同名不同内容**的文件会拿到两个不同的标识，
/// 名字撞车就得改名；这里带上名字，同一个名字重复上传也能各归各的。
fn file_id(sha256: &str, name: &str) -> String {
    hash_hex(format!("{sha256}:{name}:{}", now()).as_bytes())
        .chars()
        .take(16)
        .collect()
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

/// 后缀：小写，只留 1–8 个 ASCII 字母数字（`tar.gz` 只认最后一个）
fn extension_of(name: &str) -> String {
    let Some((_, extension)) = name.rsplit_once('.') else {
        return String::new();
    };
    let extension = extension.to_lowercase();
    if !extension.is_empty()
        && extension.len() <= 8
        && extension.chars().all(|ch| ch.is_ascii_alphanumeric())
    {
        extension
    } else {
        String::new()
    }
}

/// 同名不同内容的附件不覆盖，排到 `名字-1.png` 去
fn unique_name(table: &FileTable, wanted: &str) -> String {
    if !table.items.iter().any(|item| item.name == wanted) {
        return wanted.to_string();
    }

    let (stem, extension) = match wanted.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() => (stem.to_string(), format!(".{extension}")),
        _ => (wanted.to_string(), String::new()),
    };

    for index in 1..10_000 {
        let candidate = format!("{stem}-{index}{extension}");
        if !table.items.iter().any(|item| item.name == candidate) {
            return candidate;
        }
    }
    format!("{stem}-{}", now())
}

/// 后缀 → MIME。认不出的一律当二进制流：宁可让浏览器不预览，也不要猜错类型。
pub fn mime_of(extension: &str) -> &'static str {
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
}

/// 解码一个 URL 里的键。**宽容**：解不开就按原样用（表里去查，查不到就是没有）。
pub fn decode_key(key: &str) -> String {
    percent_decode(key)
}

/// 只认 `%XX` 的手写解码：不做 `+` → 空格那种表单语义
fn percent_decode(text: &str) -> String {
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
    use super::*;

    fn scratch(name: &str) -> Database {
        let dir = std::env::temp_dir().join(format!(
            "refind-note-files-test-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        let workspace = crate::workspace::Workspace::open(dir).unwrap();
        Database::open(&workspace).unwrap()
    }

    fn cleanup(database: &Database) {
        if let Some(root) = database.root().parent() {
            let _ = fs::remove_dir_all(root);
        }
    }

    #[test]
    fn an_uploaded_file_lands_in_the_table_and_on_disk() {
        let database = scratch("upload");
        let uploaded = database
            .add_file("桥 图.png", "PNG 数据".as_bytes())
            .unwrap();

        assert!(!uploaded.reused);
        assert_eq!(uploaded.entry.name, "桥 图.png");
        assert_eq!(uploaded.entry.mime, "image/png");
        assert_eq!(uploaded.entry.size, "PNG 数据".len() as u64);
        assert!(
            database.file_path("桥 图.png").is_some(),
            "按键（名字）找得到"
        );
        assert!(
            database.file_path(&uploaded.entry.id).is_some(),
            "按键（标识）也找得到"
        );

        let listed = database.list_files().unwrap();
        assert_eq!(listed.len(), 1);
        cleanup(&database);
    }

    #[test]
    fn the_same_bytes_under_the_same_extension_are_reused() {
        let database = scratch("dedupe");
        let first = database
            .add_file("图.png", "同样的字节".as_bytes())
            .unwrap();
        let again = database
            .add_file("另起的名字.png", "同样的字节".as_bytes())
            .unwrap();

        assert!(again.reused, "内容与后缀都一样，复用已有那一份");
        assert_eq!(again.entry.id, first.entry.id);
        assert_eq!(database.list_files().unwrap().len(), 1);
        cleanup(&database);
    }

    #[test]
    fn a_different_file_with_the_same_name_gets_a_suffix() {
        let database = scratch("collide");
        database.add_file("图.png", "第一份".as_bytes()).unwrap();
        let second = database.add_file("图.png", "第二份".as_bytes()).unwrap();

        assert_eq!(second.entry.name, "图-1.png");
        assert_eq!(database.list_files().unwrap().len(), 2);
        cleanup(&database);
    }

    #[test]
    fn only_the_name_part_survives_and_dots_are_trimmed() {
        let database = scratch("names");
        let uploaded = database
            .add_file("/tmp/某个目录/…/照片.jpg", "字节".as_bytes())
            .unwrap();
        assert_eq!(
            uploaded.entry.name, "照片.jpg",
            "路径部分与开头的点都该去掉"
        );

        assert!(database.add_file("...", "字节".as_bytes()).is_err());
        assert!(database.add_file("", "字节".as_bytes()).is_err());
        cleanup(&database);
    }

    #[test]
    fn renaming_touches_neither_the_bytes_nor_the_notes() {
        let database = scratch("rename");
        let uploaded = database.add_file("旧名.png", "字节".as_bytes()).unwrap();
        let before = database.file_path(&uploaded.entry.id);

        let renamed = database
            .rename_file(&uploaded.entry.id, "新名.png")
            .unwrap();
        assert_eq!(renamed.name, "新名.png");
        assert_eq!(renamed.extension, "png", "后缀跟着内容走，改名不动它");
        assert_eq!(
            database.file_path(&uploaded.entry.id),
            before,
            "磁盘上纹丝不动"
        );
        assert!(database.find_file("旧名.png").is_none());
        assert!(database.find_file("新名.png").is_some());

        // 撞名要报错，不能悄悄覆盖
        database.add_file("别的.png", "另一份".as_bytes()).unwrap();
        assert!(database
            .rename_file(&uploaded.entry.id, "别的.png")
            .is_err());
        cleanup(&database);
    }

    #[test]
    fn deleting_removes_the_row_then_the_bytes() {
        let database = scratch("delete");
        let uploaded = database.add_file("图.png", "字节".as_bytes()).unwrap();
        let path = database.file_path(&uploaded.entry.id).unwrap();

        database.delete_file(&uploaded.entry.id).unwrap();
        assert!(!path.is_file());
        assert!(database.list_files().unwrap().is_empty());
        assert!(
            database.delete_file(&uploaded.entry.id).is_err(),
            "再删一次要说没有"
        );
        cleanup(&database);
    }

    #[test]
    fn path_tricks_never_reach_the_disk() {
        let database = scratch("traversal");
        database.add_file("图.png", "字节".as_bytes()).unwrap();

        for key in ["..", "../vault.json", "..%2Fvault.json", "", "图.png/../x"] {
            assert!(
                database.file_path(key).is_none(),
                "{key:?} 不该找到任何东西"
            );
        }
        cleanup(&database);
    }

    #[test]
    fn oversize_files_are_refused_by_size() {
        let database = scratch("oversize");
        let error = database
            .add_file("大.png", &vec![0u8; (MAX_FILE_BYTES + 1) as usize])
            .unwrap_err();
        assert!(error.contains("文件太大"), "{error}");
        cleanup(&database);
    }

    #[test]
    fn the_extension_decides_the_mime_and_unknown_ones_are_binary() {
        let database = scratch("mime");
        assert_eq!(
            database.add_file("a.png", b"1").unwrap().entry.mime,
            "image/png"
        );
        assert_eq!(
            database
                .add_file("a.unknownthing", b"2")
                .unwrap()
                .entry
                .mime,
            "application/octet-stream"
        );
        assert_eq!(
            database.add_file("没有后缀", b"3").unwrap().entry.mime,
            "application/octet-stream"
        );
        cleanup(&database);
    }
}
