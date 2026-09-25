//! 仓库：事件日志、内容寻址、索引缓存、读写与草稿。
//!
//! 设计要点（来自需求）：
//!
//! - **每篇笔记一条独立的追加式日志**，不套用 git 的仓库级分支；相同内容只存一份
//!   （内容寻址 blob）。
//! - **草稿也接在同一条链上**，有自己的版本号；提交用 `supersedes` 记录它取代了
//!   哪些草稿节点。`parent` **永远指向上一个提交**（绝不指向草稿），因此草稿被删
//!   也绝不会断链 —— 这就是「提交能完全代替草稿节点，草稿可随时删掉」的机制。
//! - **仓库可以完全重放**：状态由日志折叠得到；`index.json` 只是缓存，删掉或损坏
//!   都能重建。默认不主动清理日志（历史留着），`prune` 是随时可做的维护操作。
//! - 存储操作刻意保持 S3 的形状（按 key 存取、只追加、不 rename），将来换真 S3 时
//!   由「每个 key 的版本列表」直接对应。

use crate::markdown;
use crate::title::{LinkResolver, NamespaceTable, ParsedTitle};

mod atomic;
mod api;
mod config;
// 增量编解码已就绪并有测试；接线进读写路径是下一步，所以先允许「暂未使用」
#[allow(dead_code)]
mod delta;
mod diff;
mod error;
mod event;

pub use api::{
    Address, DiffResult, Draft, GcReport, LoadOutcome, Note, NoteSummary, RevisionContent, RevisionSummary,
    VaultSettings,
};
pub use config::VaultConfig;
pub use error::VaultError;
pub use event::{
    drafts_of, fold, next_rev, revision_id, revision_of, short_revision_id, Event,
};

use atomic::{append_line, hash_bytes, write_atomic, BlobStore};
use std::collections::{BTreeMap, HashSet};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// 仓库目录名（放在用户主目录下）
pub const DEFAULT_DIR_NAME: &str = ".refind-note";
pub const DEFAULT_MIME: &str = "text/markdown";
/// 笔记文件的扩展名
const LOG_EXT: &str = "log";
/// 目前只有主命名空间。多留这一层目录，是为了以后允许用户创建命名空间时
/// 不必迁移已有文件。
const MAIN_NAMESPACE_DIR: &str = "0";

// ---------------------------------------------------------------- 仓库

/// 名字表：**文件名是 id，名字存在这里**。
///
/// 为什么不把标题当文件名：不同操作系统与文件系统对名字的限制、规范化方式都不一样
/// （大小写、Unicode 规范化、保留字符、长度上限…）。id 是纯 ASCII 十六进制，哪儿都一样；
/// 名字只存在于这个 JSON 里，**改名也就只是改这里一行**（不必搬文件）。
///
/// 分工：目录是「存在与否」的真相，这张表是「叫什么」的真相。两边不一致时以目录为准
/// （表里多余的条目忽略；目录里没登记的按 id 显示 —— 至少还能打开它、给它改名）。
#[derive(Debug, Default, Serialize, Deserialize)]
struct Titles {
    /// id → 标题
    notes: BTreeMap<String, String>,
    /// id → 标题（已删除，文件在 trash/ 里）
    trashed: BTreeMap<String, String>,
}

/// 一次写入的结果：选了哪种存法、落在哪个 blob 上
struct Stored {
    blob: String,
    encoding: &'static str,
    base_rev: Option<u64>,
    content_hash: String,
}

/// 读取时允许的最大增量链深度（写入侧已有上限，这里是防损坏的兜底）
const MAX_DELTA_DEPTH: usize = 1024;

/// 引用 commit ID 时至少要写的位数
const MIN_SHORT_ID: usize = 5;

pub struct Vault {
    root: PathBuf,
    table: Arc<NamespaceTable>,
    config: VaultConfig,
    blobs: BlobStore,
}

impl Vault {
    /// 打开（必要时初始化）仓库
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, VaultError> {
        let root = root.into();
        fs::create_dir_all(root.join("notes"))?;
        fs::create_dir_all(root.join("blobs"))?;
        fs::create_dir_all(root.join("trash"))?;

        // 名字表：不存在就写一份空的
        let titles_path = root.join("titles.json");
        if !titles_path.exists() {
            write_atomic(
                &titles_path,
                serde_json::to_string_pretty(&Titles::default())?.as_bytes(),
            )?;
        }

        let config_path = root.join("vault.json");
        let config: VaultConfig = match fs::read_to_string(&config_path) {
            Ok(text) => serde_json::from_str(&text)?,
            Err(_) => {
                let config = VaultConfig::default();
                write_atomic(&config_path, &serde_json::to_vec_pretty(&config)?)?;
                config
            }
        };

        let namespaces_path = root.join("namespaces.json");
        let table: NamespaceTable = match fs::read_to_string(&namespaces_path) {
            Ok(text) => serde_json::from_str(&text)?,
            Err(_) => {
                let table = NamespaceTable::builtin();
                write_atomic(&namespaces_path, &serde_json::to_vec_pretty(&table)?)?;
                table
            }
        };

        Ok(Self {
            blobs: BlobStore::new(root.join("blobs")),
            root,
            table: Arc::new(table),
            config,
        })
    }

    /// 默认仓库：`~/.refind-note`
    pub fn open_default() -> Result<Self, VaultError> {
        Self::open(default_root()?)
    }

    // ------------------------------------------------------------ 标题校验

    /// 只解析、不落盘：给前端一个权威的标题判定。
    ///
    /// 前端的即时检查负责手感（空、`@`、非法字符），而「冒号前缀是不是已知命名空间」
    /// 这类判断依赖命名空间表，前端不复制一份 —— 那会变成第二个真相来源。
    pub fn validate_title(&self, title: &str) -> Result<(), VaultError> {
        self.table
            .parse(title, self.config.capital_links)
            .map(|_| ())
            .map_err(VaultError::from)
    }

    // ------------------------------------------------------------ 路径
    //
    // 目前只有主命名空间，路径是 `<root>/notes/0/<转义标题>.log`：
    // **文件系统本身就是索引** —— 标题直接算出路径，不必先去查一张全局表。

    fn notes_dir(&self) -> PathBuf {
        self.root.join("notes")
    }

    fn trash_dir(&self) -> PathBuf {
        self.root.join("trash")
    }

    /// 笔记文件路径。`id` 是**转义后的标题**（见 `crate::title::encode_for_path`）。
    fn log_path(&self, id: &str) -> PathBuf {
        self.notes_dir()
            .join(MAIN_NAMESPACE_DIR)
            .join(format!("{id}.{LOG_EXT}"))
    }

    /// 已删除的笔记挪到这里：历史保留，将来可以接恢复
    fn trashed_path(&self, id: &str) -> PathBuf {
        self.trash_dir()
            .join(MAIN_NAMESPACE_DIR)
            .join(format!("{id}.{LOG_EXT}"))
    }

    fn titles_path(&self) -> PathBuf {
        self.root.join("titles.json")
    }

    /// 读名字表；文件不在就当空的（首次运行或被人删掉时都还能继续）
    fn titles(&self) -> Result<Titles, VaultError> {
        match fs::read_to_string(self.titles_path()) {
            Ok(text) => Ok(serde_json::from_str(&text)?),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Titles::default()),
            Err(error) => Err(error.into()),
        }
    }

    fn save_titles(&self, titles: &Titles) -> Result<(), VaultError> {
        let text = serde_json::to_string_pretty(titles)?;
        write_atomic(&self.titles_path(), text.as_bytes())?;
        Ok(())
    }

    /// 路径 → id（文件名就是 id）
    fn id_from_path(path: &Path) -> String {
        path.file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or_default()
            .to_string()
    }

    /// 生成一个还没被用过的 id：纯 ASCII 十六进制，撞了就再取一个
    fn new_id(&self, titles: &Titles) -> String {
        let mut salt = 0u64;
        loop {
            let seed = format!("{}:{}", now_iso(), salt);
            let id: String = hash_bytes(seed.as_bytes()).chars().take(16).collect();
            if !titles.notes.contains_key(&id)
                && !titles.trashed.contains_key(&id)
                && !self.log_path(&id).exists()
            {
                return id;
            }
            salt += 1;
        }
    }

    /// 由标题定位：**先查名字表**（登记过就用它的 id —— 改名不换 id、文件不搬家）；
    /// 没登记就生成一个新 id。新 id **不写表**：只有真正落盘时才登记，免得看一眼不存在的
    /// 笔记就留下幽灵条目。
    fn locate(&self, title: &str) -> Result<(ParsedTitle, PathBuf), VaultError> {
        let parsed = self.table.parse(title, self.config.capital_links)?;
        let display = parsed.display(&self.table);
        let titles = self.titles()?;
        // 名字表是「id → 标题」，所以必须**按值反查**；按键查永远查不到。
        // 已删除的（在 trashed 里）也要算进来：它的 id 还要用来判断
        // 「是删过，还是从来没有过」（load_outcome 会去看 trash/）。
        let id = titles
            .notes
            .iter()
            .chain(titles.trashed.iter())
            .find(|(_, title)| *title == &display)
            .map(|(id, _)| id.clone())
            .unwrap_or_else(|| self.new_id(&titles));
        Ok((parsed, self.log_path(&id)))
    }

    /// 目录里现有的 id（文件名的真相；GC 与 prune 用）
    fn walk_ids(&self, base: &Path) -> Result<Vec<String>, VaultError> {
        let mut out = Vec::new();
        let namespaces = match fs::read_dir(base) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(out),
            Err(error) => return Err(error.into()),
        };

        for namespace_entry in namespaces {
            let namespace_path = namespace_entry?.path();
            if !namespace_path.is_dir() {
                continue;
            }
            for entry in fs::read_dir(&namespace_path)? {
                let path = entry?.path();
                if path.extension().and_then(|ext| ext.to_str()) != Some(LOG_EXT) {
                    continue;
                }
                if let Some(id) = path.file_stem().and_then(|stem| stem.to_str()) {
                    out.push(id.to_string());
                }
            }
        }

        Ok(out)
    }

    // ------------------------------------------------------------ 日志

    /// 读某个路径下的事件流。崩溃可能留下半行，跳过而不是整体失败。
    fn read_events_at(&self, path: &Path) -> Result<Vec<Event>, VaultError> {
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };

        let mut events = Vec::new();
        for line in text.lines() {
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<Event>(line) {
                Ok(event) => events.push(event),
                // 崩溃可能留下半行，跳过而不是整体失败
                Err(error) => eprintln!("[vault] 跳过无法解析的日志行：{error}"),
            }
        }
        Ok(events)
    }

    fn read_events(&self, id: &str) -> Result<Vec<Event>, VaultError> {
        self.read_events_at(&self.log_path(id))
    }

    /// 写入前的碰撞防线。
    ///
    /// 版本 ID 由事件自身派生，理论上可能撞车（哈希被攻破、派生规则出 bug、
    /// 时间戳精度被改写…）。所以**宁可拒绝写入**，也不要让仓库里出现两个
    /// 「同一个 ID」的版本 —— 那样 ID 就不再是版本的身份了。
    fn ensure_unique_id(&self, event: &Event) -> Result<(), VaultError> {
        let id = revision_id(event);
        for (_, path) in self.log_files()? {
            for existing in self.read_events_at(&path)? {
                if revision_id(&existing) == id {
                    return Err(VaultError::IdCollision(short_revision_id(&id)));
                }
            }
        }
        Ok(())
    }

    fn append(&self, id: &str, event: &Event) -> Result<(), VaultError> {
        self.ensure_unique_id(event)?;
        let line = serde_json::to_string(event)?;
        append_line(&self.log_path(id), &line)?;
        Ok(())
    }

    /// 重写日志（只用于 prune / 丢弃草稿这类显式维护操作）
    fn rewrite_log(&self, id: &str, keep: impl Fn(&Event) -> bool) -> Result<usize, VaultError> {
        let path = self.log_path(id);
        let events = self.read_events_at(&path)?;
        let total = events.len();
        let kept: Vec<&Event> = events.iter().filter(|event| keep(event)).collect();
        let removed = total - kept.len();
        if removed == 0 {
            return Ok(0);
        }

        let mut body = String::new();
        for event in kept {
            body.push_str(&serde_json::to_string(event)?);
            body.push('\n');
        }
        write_atomic(&path, body.as_bytes())?;
        Ok(removed)
    }

    // ------------------------------------------------------------ 遍历
    //
    // 文件名就是标题，所以「列出全部笔记」只是列目录 —— 不打开任何文件。
    // 列表、红蓝链判定、首次运行判断都靠它。

    /// 扫出一个目录下现有的笔记（只列目录，不读内容）
    fn walk(&self, base: &Path) -> Result<Vec<ParsedTitle>, VaultError> {
        let titles = self.titles()?;
        let mut out = Vec::new();

        let namespaces = match fs::read_dir(base) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(out),
            Err(error) => return Err(error.into()),
        };

        for namespace_entry in namespaces {
            let namespace_path = namespace_entry?.path();
            if !namespace_path.is_dir() {
                continue;
            }

            let Some(ns) = namespace_path
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| name.parse::<i32>().ok())
            else {
                continue;
            };

            for entry in fs::read_dir(&namespace_path)? {
                let path = entry?.path();
                if path.extension().and_then(|ext| ext.to_str()) != Some(LOG_EXT) {
                    continue;
                }
                let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
                    continue;
                };
                // 文件名是 id，标题去名字表里查；没登记的按 id 显示（还能打开它、改名）
                let title = titles
                    .notes
                    .get(stem)
                    .or_else(|| titles.trashed.get(stem))
                    .cloned()
                    .unwrap_or_else(|| stem.to_string());
                out.push(ParsedTitle { ns, title });
            }
        }

        Ok(out)
    }

    fn note_ids(&self) -> Result<Vec<String>, VaultError> {
        self.walk_ids(&self.notes_dir())
    }

    /// 某篇笔记的全部事件（测试助手）
    #[cfg(test)]
    pub(crate) fn events_for(&self, title: &str) -> Result<Vec<Event>, VaultError> {
        let (_, path) = self.locate(title)?;
        self.read_events(&Self::id_from_path(&path))
    }

    // ------------------------------------------------------------ 链接解析

    /// 构造渲染用的解析器：键集合来自目录遍历（没有全局索引可查）
    fn resolver(&self, from: Option<ParsedTitle>) -> LinkResolver {
        let keys: HashSet<String> = self
            .walk(&self.notes_dir())
            .map(|items| items.iter().map(|parsed| parsed.key()).collect())
            .unwrap_or_default();

        LinkResolver::new(
            Arc::clone(&self.table),
            Arc::new(keys),
            self.config.capital_links,
            from,
        )
    }

    // ------------------------------------------------------------ 设置

    /// 仓库级设置（影响数据语义的那些）
    pub fn settings_view(&self) -> VaultSettings {
        VaultSettings {
            root: self.root.display().to_string(),
            format: self.config.format,
            capital_links: self.config.capital_links,
            max_title_bytes: self.config.max_title_bytes,
        }
    }

    /// 改仓库级设置。只接受明确给出的字段，其余保持不变。
    pub fn update_settings(
        &mut self,
        capital_links: Option<bool>,
        max_title_bytes: Option<usize>,
    ) -> Result<(), VaultError> {
        if let Some(value) = capital_links {
            self.config.capital_links = value;
        }
        if let Some(value) = max_title_bytes {
            self.config.max_title_bytes = value;
        }
        let path = self.root.join("vault.json");
        write_atomic(&path, &serde_json::to_vec_pretty(&self.config)?)?;
        Ok(())
    }

    // ------------------------------------------------------------ 内容存储
    //
    // 一版内容可以整份存，也可以只存「相对某个基准的补丁」。三条规则：
    // 1. 增量**不见得更小**（大改动要把新内容整段塞进去），所以真比一次再决定；
    // 2. 增量可以递归依赖增量，但链太长读取会慢 —— 超过上限就退回整份快照；
    // 3. 提交的基准**只能是提交**（不能依赖草稿），草稿一律整份存；
    // 4. 二进制（非文本）一律整份存。



    /// 某个版本所在的增量链有多长（从最近一次整份快照算起）
    fn chain_length(&self, events: &[Event], rev: u64) -> usize {
        let mut length = 0usize;
        let mut cursor = Some(rev);

        while let Some(current) = cursor {
            let Some(event) = events.iter().find(|event| match event {
                Event::Rev { rev, .. } | Event::Auto { rev, .. } => *rev == current,
                _ => false,
            }) else {
                break;
            };

            let (encoding, base) = match event {
                Event::Rev {
                    encoding, base_rev, ..
                }
                | Event::Auto {
                    encoding, base_rev, ..
                } => (encoding.as_str(), *base_rev),
                _ => break,
            };

            if encoding != "delta" {
                break;
            }
            length += 1;
            cursor = base;
        }

        length
    }

    /// 决定这一版怎么存：整份快照，还是相对基准的增量
    fn store_content(
        &self,
        events: &[Event],
        base: Option<(u64, String)>,
        mime: &str,
        content: &str,
    ) -> Result<Stored, VaultError> {
        let content_hash = hash_bytes(content.as_bytes());

        if mime == DEFAULT_MIME {
            if let Some((base_rev, base_text)) = base {
                if self.chain_length(events, base_rev) < self.config.delta_chain_limit {
                    let ops = delta::encode(&base_text, content);
                    let patch = delta::encode_bytes(&ops);

                    // 规则一：跟完整内容真比一次，补丁更大就用整份
                    if patch.len() < content.len() {
                        let blob = self.blobs.put(&patch)?;
                        return Ok(Stored {
                            blob,
                            encoding: "delta",
                            base_rev: Some(base_rev),
                            content_hash,
                        });
                    }
                }
            }
        }

        let blob = self.blobs.put(content.as_bytes())?;
        Ok(Stored {
            blob,
            encoding: "full",
            base_rev: None,
            content_hash,
        })
    }

    /// 取某个版本的内容：整份直接读，增量先取基准再套补丁
    fn content_of(&self, events: &[Event], rev: u64, depth: usize) -> Result<String, VaultError> {
        if depth > MAX_DELTA_DEPTH {
            return Err(VaultError::Corrupt(format!(
                "增量链超过 {MAX_DELTA_DEPTH} 层，可能已经损坏"
            )));
        }

        let found = events.iter().find_map(|event| match event {
            Event::Rev {
                rev: item_rev,
                blob,
                encoding,
                base_rev,
                ..
            }
            | Event::Auto {
                rev: item_rev,
                blob,
                encoding,
                base_rev,
                ..
            } if *item_rev == rev => Some((blob.clone(), encoding.clone(), *base_rev)),
            _ => None,
        });

        let Some((blob, encoding, base_rev)) = found else {
            return Err(VaultError::Corrupt(format!("找不到版本 {rev} 的内容")));
        };

        let bytes = self.blobs.get(&blob)?;
        if encoding != "delta" {
            return String::from_utf8(bytes)
                .map_err(|_| VaultError::NotText(format!("版本 {rev}")));
        }

        let base_rev = base_rev
            .ok_or_else(|| VaultError::Corrupt(format!("版本 {rev} 标为增量却没有基准")))?;
        let base_text = self.content_of(events, base_rev, depth + 1)?;
        let ops = delta::decode_bytes(&bytes)
            .map_err(|error| VaultError::Corrupt(error.to_string()))?;
        delta::apply(&base_text, &ops).map_err(|error| VaultError::Corrupt(error.to_string()))
    }

    // ------------------------------------------------------------ 读

    pub fn list_notes(&self) -> Result<Vec<NoteSummary>, VaultError> {
        let mut out: Vec<NoteSummary> = self
            .walk(&self.notes_dir())?
            .into_iter()
            .map(|parsed| NoteSummary {
                key: parsed.key(),
                title: parsed.display(&self.table),
            })
            .collect();
        out.sort_by(|a, b| a.title.cmp(&b.title));
        Ok(out)
    }

    /// 读一篇笔记。**目标不存在不算错误**，而是返回一个交给界面处理的结果
    /// （前端据此显示「还没有这篇笔记」与创建按钮）。
    pub fn load_outcome(&self, title: &str) -> Result<LoadOutcome, VaultError> {
        let (parsed, path) = self.locate(title)?;
        let display = parsed.display(&self.table);
        let id = Self::id_from_path(&path);

        if !path.is_file() {
            // 被删除过的笔记躺在 trash/ 里；区分开是为了将来能接恢复
            return Ok(LoadOutcome {
                note: None,
                title: display,
                deleted: self.trashed_path(&id).is_file(),
            });
        }

        let events = self.read_events(&id)?;
        let state = fold(&events);
        if state.deleted {
            return Ok(LoadOutcome {
                note: None,
                title: display,
                deleted: true,
            });
        }

        // rev 0 是「建了但还没提交」，内容为空；否则按版本回放（可能是增量）
        let markdown_text = if state.rev == 0 {
            String::new()
        } else {
            self.content_of(&events, state.rev, 0)?
        };

        let resolver = self.resolver(Some(parsed.clone()));
        let html = markdown::render_with(&markdown_text, Some(&resolver));

        Ok(LoadOutcome {
            note: Some(Note {
                key: parsed.key(),
                title: parsed.display(&self.table),
                markdown: markdown_text,
                html,
                rev: state.rev,
                modified: state.at,
            }),
            title: display,
            deleted: false,
        })
    }


    pub fn load(&self, title: &str) -> Result<Note, VaultError> {
        let outcome = self.load_outcome(title)?;
        match outcome.note {
            Some(note) => Ok(note),
            None if outcome.deleted => Err(VaultError::Deleted(outcome.title)),
            None => Err(VaultError::NotFound(outcome.title)),
        }
    }

    // ------------------------------------------------------------ 历史

    /// 版本历史：只回元信息（大小、摘要、类型），正文用 [`Self::revision`] 按需取。
    ///
    /// 草稿也在版本序列里，所以会一并列出（`kind` 是 `draft`），由界面决定是否显示。
    pub fn history(&self, title: &str) -> Result<Vec<RevisionSummary>, VaultError> {
        let (_parsed, path) = self.locate(title)?;
        let id = Self::id_from_path(&path);
        let events = self.read_events(&id)?;

        let mut out = Vec::new();
        let mut previous_bytes = 0u64;

        for event in &events {
            let (rev, kind, at, bytes, supersedes, summary, encoding) = match event {
                Event::Meta { at, .. } => {
                    (0, "create", at.clone(), 0u64, Vec::new(), None, "full")
                }
                Event::Rev {
                    at,
                    rev,
                    bytes,
                    encoding,
                    supersedes,
                    summary,
                    ..
                } => (
                    *rev,
                    "commit",
                    at.clone(),
                    *bytes,
                    supersedes.clone(),
                    summary.clone(),
                    encoding.as_str(),
                ),
                Event::Auto {
                    at,
                    rev,
                    bytes,
                    encoding,
                    on,
                    ..
                } => (
                    *rev,
                    "draft",
                    at.clone(),
                    *bytes,
                    Vec::new(),
                    Some(format!("自动保存（基于版本 {on}）")),
                    encoding.as_str(),
                ),
                Event::Del { at, rev, summary } => (
                    *rev,
                    "delete",
                    at.clone(),
                    0u64,
                    Vec::new(),
                    summary.clone(),
                    "full",
                ),
            };

            // 只有带正文的记录才谈得上「比上一条多了多少字节」
            let id = revision_id(event);
            let has_content = matches!(event, Event::Rev { .. } | Event::Auto { .. });
            let delta = if has_content {
                bytes as i64 - previous_bytes as i64
            } else {
                0
            };
            if has_content {
                previous_bytes = bytes;
            }

            out.push(RevisionSummary {
                rev,
                kind: kind.to_string(),
                at,
                bytes,
                delta,
                encoding: encoding.to_string(),
                id: id.clone(),
                short_id: short_revision_id(&id),
                supersedes,
                summary,
            });
        }

        Ok(out)
    }

    /// 取某一个版本的正文。
    ///
    /// 标题按**该版本当时**的算：改名之前的版本要显示旧标题，否则历史会看起来
    /// 像是「所有版本都叫现在这个名字」。
    pub fn revision(&self, title: &str, rev: u64) -> Result<RevisionContent, VaultError> {
        let (parsed, path) = self.locate(title)?;
        let id = Self::id_from_path(&path);
        let events = self.read_events(&id)?;

        let mut ns = 0;
        let mut page = String::new();
        // (类型, 时间, 内容哈希)
        let mut found: Option<(&str, String, String)> = None;

        for event in &events {
            match event {
                Event::Meta {
                    ns: meta_ns,
                    title: meta_title,
                    ..
                } => {
                    ns = *meta_ns;
                    page = meta_title.clone();
                }
                Event::Rev {
                    at,
                    rev: item_rev,
                    blob,
                    ns: item_ns,
                    title: item_title,
                    ..
                } => {
                    if let Some(value) = item_ns {
                        ns = *value;
                    }
                    if let Some(value) = item_title {
                        page = value.clone();
                    }
                    if *item_rev == rev {
                        found = Some(("commit", at.clone(), blob.clone()));
                        // 找到就停：再往后走，改名会把这一版当时的标题覆盖掉
                        break;
                    }
                }
                Event::Auto {
                    at,
                    rev: item_rev,
                    blob,
                    ..
                } => {
                    if *item_rev == rev {
                        found = Some(("draft", at.clone(), blob.clone()));
                    }
                }
                Event::Del {
                    at,
                    rev: item_rev,
                    ..
                } => {
                    if *item_rev == rev {
                        found = Some(("delete", at.clone(), String::new()));
                    }
                }
            }
        }

        let Some((kind, at, blob)) = found else {
            return Err(VaultError::RevisionNotFound {
                title: parsed.title.clone(),
                rev,
            });
        };
        if blob.is_empty() {
            return Err(VaultError::RevisionNotFound {
                title: parsed.title.clone(),
                rev,
            });
        }

        let parsed = ParsedTitle { ns, title: page };
        let markdown_text = self.content_of(&events, rev, 0)?;
        let resolver = self.resolver(Some(parsed.clone()));
        let html = markdown::render_with(&markdown_text, Some(&resolver));

        Ok(RevisionContent {
            rev,
            kind: kind.to_string(),
            at,
            title: parsed.display(&self.table),
            markdown: markdown_text,
            html,
        })
    }





    /// 解析地址栏那一行，并**解析到底**。
    ///
    /// 页面地址模型是 `NAMESPACE:NAME@STATE`，STATE 是枚举：
    ///
    /// - `view-版本` —— 看某一版（**默认状态**：看最新提交时整个 STATE 省略）
    /// - `edit` / `history` / `delete` —— 编辑 / 版本历史 / 删除二次确认
    /// - `rollback-版本` —— 回退二次确认
    ///
    /// 命名空间默认 0 号（主命名空间），同样省略；末尾可接 `#章节`（章节是唯一允许前端
    /// 自己确定的成分）。**回显一律最简**：默认状态不写、版本写缩写 —— 所以
    /// `名称@view-1` 会回显成 `名称@view-<缩写>`，而最简单的笔记就是 `名称`。
    ///
    /// 版本在**这篇笔记内**解析（数字版本号或 commit ID 缩写都行）。
    pub fn parse_address(&self, input: &str) -> Result<Address, VaultError> {
        let raw = input.trim();
        if raw.is_empty() {
            return Ok(Address::Empty);
        }

        // 虚拟命名空间 `special:`：不对应笔记文件，交给前端渲染特殊页面。
        // 优先于笔记形态判断；标题里不允许冒号，所以不可能有笔记叫这个名字。
        // ⚠️ 这里必须用 get(..n) 而不是 &raw[..n]：后者按**字节**切，中文标题（如
        // 「平陆运河」12 字节）会让切口落在字符中间直接 panic；而这个 panic 发生在
        // GTK 回调里不能 unwind，会把整个应用 abort。
        if let Some(rest) = strip_prefix_ci(raw, "special:") {
            let page = rest
                .split('#')
                .next()
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase();

            if page.is_empty() {
                return Err(VaultError::BadAddress(
                    "special: 后面要写页面名，例如 special:newtab".to_string(),
                ));
            }

            return Ok(Address::Special {
                address: format!("special:{page}"),
                page,
            });
        }

        // 切分：NAME 之外的成分各由保留字符开头，所以顺序天然自由
        let name_end = raw
            .char_indices()
            .find(|(_, ch)| matches!(ch, '@' | '#'))
            .map(|(index, _)| index)
            .unwrap_or(raw.len());
        let name_part = raw[..name_end].trim();
        let rest = &raw[name_end..];

        let mut state: Option<String> = None;
        let mut section: Option<String> = None;
        let mut cursor = 0;
        while cursor < rest.len() {
            let marker = rest[cursor..].chars().next().expect("非空");
            let from = cursor + marker.len_utf8();
            let to = rest[from..]
                .find(['@', '#'])
                .map(|offset| from + offset)
                .unwrap_or(rest.len());
            let value = rest[from..to].trim();

            match marker {
                '@' => state = Some(value.to_string()),
                '#' => section = Some(value.to_string()),
                _ => {}
            }
            cursor = to;
        }
        let section = section.filter(|value| !value.is_empty());

        let parsed = self.table.parse(name_part, self.config.capital_links)?;
        let display = parsed.display(&self.table);
        // 同样的反查：表是 id → 标题
        let Some(id) = self
            .titles()?
            .notes
            .iter()
            .find(|(_, title)| *title == &display)
            .map(|(id, _)| id.clone())
        else {
            return Ok(Address::Missing {
                address: compose_address(&display, None, section.as_deref()),
                title: display,
            });
        };

        let state = state.unwrap_or_default();
        let (head, reference) = match state.split_once('-') {
            Some((head, reference)) => (head.to_ascii_lowercase(), Some(reference.trim())),
            None => (state.to_ascii_lowercase(), None),
        };

        match (head.as_str(), reference) {
            // 默认：看最新提交
            ("", _) | ("view", None) => Ok(Address::Note {
                address: compose_address(&display, None, section.as_deref()),
                title: display,
            }),
            ("edit", None) => Ok(Address::Edit {
                address: compose_address(&display, Some("edit"), section.as_deref()),
                title: display,
            }),
            ("history", None) => Ok(Address::History {
                address: compose_address(&display, Some("history"), section.as_deref()),
                title: display,
            }),
            ("delete", None) => Ok(Address::Delete {
                address: compose_address(&display, Some("delete"), section.as_deref()),
                title: display,
            }),
            ("view" | "rollback", Some(reference)) if !reference.is_empty() => {
                let rev = self.resolve_revision(&display, reference)?;
                let events = self.read_events(&id)?;
                let event = events
                    .iter()
                    .find(|event| revision_of(event) == Some(rev))
                    .ok_or_else(|| VaultError::RevisionNotFound {
                        title: display.clone(),
                        rev,
                    })?;
                let full_id = revision_id(event);
                let short = short_revision_id(&full_id);
                // 规则：看的就是**最新提交**时，`view-` 正是默认状态，回显里裁掉它
                // （只裁 view —— rollback 不是默认状态，不能省）。
                if head == "view" && rev == fold(&events).rev {
                    return Ok(Address::Note {
                        address: compose_address(&display, None, section.as_deref()),
                        title: display,
                    });
                }

                let canonical = format!("{head}-{short}");

                // 先算好地址，再构造（否则 display 会先被移进 title、后面又借用）
                let address =
                    compose_address(&display, Some(&canonical), section.as_deref());

                Ok(if head == "rollback" {
                    Address::RollbackConfirm {
                        title: display,
                        rev,
                        id: full_id,
                        short_id: short,
                        address,
                    }
                } else {
                    Address::ViewVersion {
                        title: display,
                        rev,
                        id: full_id,
                        short_id: short,
                        address,
                    }
                })
            }
            ("rollback", None) => Err(VaultError::BadAddress(
                "回退要指出哪一版：写成 rollback-版本".to_string(),
            )),
            _ => Err(VaultError::BadAddress(format!(
                "不认识「@{state}」；状态只有 edit / history / delete / view-版本 / rollback-版本"
            ))),
        }
    }

    /// 把「版本引用」解析成版本号。
    ///
    /// 接受两种写法（与 git 一致的地方就在第二种）：
    /// - 纯数字：就是版本号本身；
    /// - 十六进制缩写：commit ID 的前缀，撞上多个就报歧义、绝不猜。
    pub fn resolve_revision(&self, title: &str, reference: &str) -> Result<u64, VaultError> {
        let (parsed, path) = self.locate(title)?;
        let events = self.read_events(&Self::id_from_path(&path))?;
        let reference = reference.trim();

        if reference.is_empty() {
            return Err(VaultError::RevisionNotFound {
                title: parsed.title.clone(),
                rev: 0,
            });
        }

        let digits = reference.chars().all(|ch| ch.is_ascii_digit());

        // 不足 5 位：只能当版本号。缩写按约定至少 5 位，太短给明确提示。
        if reference.len() < MIN_SHORT_ID {
            if digits {
                return reference
                    .parse::<u64>()
                    .map_err(|_| VaultError::RevisionNotFound {
                        title: parsed.title.clone(),
                        rev: 0,
                    });
            }
            return Err(VaultError::BadAddress(format!(
                "commit ID 缩写至少要写 {MIN_SHORT_ID} 位"
            )));
        }

        // 5 位以上：**先按 ID 前缀找**，找不到再退回数字版本号。
        //
        // 顺序很要紧：8 位十六进制缩写里约 2% 会恰好全是数字，若先按「纯数字」
        // 判断，就会把这种缩写误当成版本号（曾经偶发把 rev 解析成八位数）。
        let needle = reference.to_ascii_lowercase();
        let mut matches: Vec<u64> = Vec::new();
        for event in &events {
            let Some(rev) = revision_of(event) else {
                continue;
            };
            if revision_id(event).starts_with(&needle) {
                matches.push(rev);
            }
        }

        match matches.len() {
            1 => Ok(matches[0]),
            0 if digits => reference
                .parse::<u64>()
                .map_err(|_| VaultError::RevisionNotFound {
                    title: parsed.title.clone(),
                    rev: 0,
                }),
            0 => Err(VaultError::RevisionNotFound {
                title: parsed.title.clone(),
                rev: 0,
            }),
            count => Err(VaultError::AmbiguousRevision {
                prefix: reference.to_string(),
                matches: count,
            }),
        }
    }

    /// 对比两个版本，逐行返回差异
    pub fn compare(&self, title: &str, from: u64, to: u64) -> Result<DiffResult, VaultError> {
        let from_content = self.revision(title, from)?;
        let to_content = self.revision(title, to)?;

        Ok(diff::line_diff(
            from,
            &from_content.title,
            &from_content.markdown,
            to,
            &to_content.title,
            &to_content.markdown,
        ))
    }

    // ------------------------------------------------------------ 写

    /// 建立一篇新笔记（内容为空，版本 0）
    pub fn create(&self, title: &str) -> Result<Note, VaultError> {
        let (parsed, path) = self.locate(title)?;
        if path.is_file() {
            return self.load(title);
        }

        // 先登记名字再写事件：之后再按 id 就能找到它
        let mut titles = self.titles()?;
        titles
            .notes
            .insert(Self::id_from_path(&path), parsed.display(&self.table));
        self.save_titles(&titles)?;

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        self.append(
            &Self::id_from_path(&path),
            &Event::Meta {
                at: now_iso(),
                ns: parsed.ns,
                title: parsed.title.clone(),
            },
        )?;
        self.load(title)
    }

    /// 提交：只有它会把内容写进版本链
    pub fn commit(
        &self,
        title: &str,
        markdown_text: &str,
        summary: Option<&str>,
        base_rev: u64,
    ) -> Result<Note, VaultError> {
        let (parsed, path) = self.locate(title)?;
        let id = Self::id_from_path(&path);

        // 文件不存在就是第一次提交，顺手把 meta 写上
        let events = if path.is_file() {
            self.read_events(&id)?
        } else {
            // 第一次提交：把名字登记进名字表
            let mut titles = self.titles()?;
            titles
                .notes
                .insert(id.clone(), parsed.display(&self.table));
            self.save_titles(&titles)?;

            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            self.append(
                &id,
                &Event::Meta {
                    at: now_iso(),
                    ns: parsed.ns,
                    title: parsed.title.clone(),
                },
            )?;
            Vec::new()
        };

        let state = fold(&events);
        if state.rev != base_rev {
            return Err(VaultError::Conflict {
                expected: base_rev,
                found: state.rev,
            });
        }

        // 增量基准取上一个**提交**的内容（绝不用草稿：草稿会被清理）
        let base = if state.rev > 0 {
            Some((state.rev, self.content_of(&events, state.rev, 0)?))
        } else {
            None
        };
        let stored = self.store_content(&events, base, DEFAULT_MIME, markdown_text)?;

        let rev = next_rev(&events);
        self.append(
            &id,
            &Event::Rev {
                at: now_iso(),
                rev,
                blob: stored.blob,
                bytes: markdown_text.len() as u64,
                encoding: stored.encoding.to_string(),
                base_rev: stored.base_rev,
                content_hash: stored.content_hash,
                mime: DEFAULT_MIME.to_string(),
                parent: (state.rev > 0).then_some(state.rev),
                // 一次提交结束了挂在当前版本上的那一串草稿
                supersedes: drafts_of(&events, state.rev),
                ns: None,
                title: None,
                summary: summary.map(str::to_string),
            },
        )?;

        self.load(title)
    }

    /// 改名。**会搬文件**（路径就是标题），但历史跟着文件走，一条都不丢。
    /// 改名。**只改名字表里那一行** —— 文件不搬家、历史不动。
    ///
    /// id 是创建时生成并登记的，与标题无关，所以改名不必碰存储：旧标题那条按键删掉，
    /// 再把同一个 id 挂到新标题上。
    pub fn rename(&self, from: &str, to: &str) -> Result<Note, VaultError> {
        let (source, source_path) = self.locate(from)?;
        if !source_path.is_file() {
            return Err(VaultError::NotFound(source.display(&self.table)));
        }

        let target = self.table.parse(to, self.config.capital_links)?;
        let source_display = source.display(&self.table);
        let target_display = target.display(&self.table);
        if source_display == target_display {
            return self.load(to);
        }

        let id = Self::id_from_path(&source_path);
        let mut titles = self.titles()?;
        if titles.notes.values().any(|title| title == &target_display) {
            return Err(VaultError::NameTaken(target_display));
        }
        titles.notes.remove(&id);
        titles.notes.insert(id.clone(), target_display.clone());

        let events = self.read_events(&id)?;
        let state = fold(&events);
        let rev = next_rev(&events);
        self.append(
            &id,
            &Event::Rev {
                at: now_iso(),
                rev,
                blob: state.blob.clone().unwrap_or_default(),
                bytes: state.bytes,
                encoding: state.encoding.clone(),
                base_rev: state.base_rev,
                content_hash: state.content_hash.clone(),
                mime: state.mime.clone(),
                parent: (state.rev > 0).then_some(state.rev),
                supersedes: drafts_of(&events, state.rev),
                ns: Some(target.ns),
                title: Some(target.title.clone()),
                summary: Some(format!("改名：{target_display}")),
            },
        )?;

        self.save_titles(&titles)?;

        self.load(&target_display)
    }

    /// 删除：写一条删除标记，然后把文件挪进 `trash/`（不抹历史）
    pub fn delete(&self, title: &str) -> Result<(), VaultError> {
        let (parsed, path) = self.locate(title)?;
        if !path.is_file() {
            return Err(VaultError::NotFound(parsed.display(&self.table)));
        }

        let id = Self::id_from_path(&path);
        let events = self.read_events(&id)?;
        self.append(
            &id,
            &Event::Del {
                at: now_iso(),
                rev: next_rev(&events),
                summary: None,
            },
        )?;

        // 名字挪到 trashed：文件进了 trash/，名字也跟着过去
        let mut titles = self.titles()?;
        titles.notes.remove(&id);
        titles
            .trashed
            .insert(id.clone(), parsed.display(&self.table));
        self.save_titles(&titles)?;

        let trashed = self.trashed_path(&id);
        if let Some(parent) = trashed.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::rename(&path, &trashed)?;
        Ok(())
    }

    // ------------------------------------------------------------ 草稿

    /// 自动保存：往链上追加一个草稿节点（不改变当前提交）
    pub fn save_draft(
        &self,
        title: &str,
        markdown_text: &str,
        base_rev: u64,
    ) -> Result<(), VaultError> {
        let id = Self::id_from_path(&self.locate(title)?.1);
        let events = self.read_events(&id)?;
        let state = fold(&events);
        if state.rev != base_rev {
            return Err(VaultError::Conflict {
                expected: base_rev,
                found: state.rev,
            });
        }

        let content_hash = hash_bytes(markdown_text.as_bytes());
        // 内容没变就不追加，免得自动保存把日志灌满
        if let Some(draft) = &state.draft {
            if draft.blob == content_hash {
                return Ok(());
            }
        }

        let rev = next_rev(&events);
        let blob = self.blobs.put(markdown_text.as_bytes())?;
        self.append(
            &id,
            &Event::Auto {
                at: now_iso(),
                rev,
                blob,
                bytes: markdown_text.len() as u64,
                // 草稿整份存：它们随时会被提交取代、被清理，不值得为增量链操心
                encoding: "full".to_string(),
                base_rev: None,
                content_hash,
                mime: DEFAULT_MIME.to_string(),
                on: base_rev,
            },
        )
    }

    pub fn load_draft(&self, title: &str) -> Result<Option<Draft>, VaultError> {
        let id = Self::id_from_path(&self.locate(title)?.1);
        let events = self.read_events(&id)?;
        let state = fold(&events);
        let Some(draft) = state.draft else {
            return Ok(None);
        };

        // 从链上找回这一版草稿的事件，好把它的 ID 一并给出去
        let event = events
            .iter()
            .rev()
            .find(|event| matches!(event, Event::Auto { blob, .. } if *blob == draft.blob))
            .ok_or_else(|| VaultError::Corrupt("草稿事件不见了".to_string()))?;
        let id = revision_id(event);

        let text = String::from_utf8(self.blobs.get(&draft.blob)?)
            .map_err(|_| VaultError::NotText(title.to_string()))?;
        Ok(Some(Draft {
            markdown: text,
            base_rev: draft.on,
            at: draft.at,
            short_id: short_revision_id(&id),
            id,
        }))
    }

    /// 丢弃当前草稿：把挂在当前版本上的草稿节点从日志里删掉
    pub fn discard_draft(&self, title: &str) -> Result<usize, VaultError> {
        let id = Self::id_from_path(&self.locate(title)?.1);
        let events = self.read_events(&id)?;
        let current = fold(&events).rev;
        self.rewrite_log(&id, |event| {
            !matches!(event, Event::Auto { on, .. } if *on == current)
        })
    }

    /// 清理已被提交取代的草稿节点。随时可做；不做也不影响正确性。
    pub fn prune(&self, title: &str) -> Result<usize, VaultError> {
        let id = Self::id_from_path(&self.locate(title)?.1);
        let events = self.read_events(&id)?;
        let superseded = fold(&events).superseded;
        self.rewrite_log(&id, |event| match event {
            Event::Auto { rev, .. } => !superseded.contains(rev),
            _ => true,
        })
    }


    /// 对全部笔记做一次清理，返回清掉的草稿节点数
    pub fn prune_all(&self) -> Result<usize, VaultError> {
        let mut removed = 0;
        for id in self.note_ids()? {
            let events = self.read_events(&id)?;
            let superseded = fold(&events).superseded;
            if superseded.is_empty() {
                continue;
            }
            removed += self.rewrite_log(&id, |event| match event {
                Event::Auto { rev, .. } => !superseded.contains(rev),
                _ => true,
            })?;
        }
        Ok(removed)
    }

    // ------------------------------------------------------------ 回收

    /// 仓库里全部笔记文件（含 trash/）
    fn log_files(&self) -> Result<Vec<(String, PathBuf)>, VaultError> {
        let mut out = Vec::new();

        for dir in [self.notes_dir(), self.trash_dir()] {
            let namespaces = match fs::read_dir(&dir) {
                Ok(entries) => entries,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            };

            for namespace_entry in namespaces {
                let namespace_path = namespace_entry?.path();
                if !namespace_path.is_dir() {
                    continue;
                }

                for entry in fs::read_dir(&namespace_path)? {
                    let path = entry?.path();
                    if path.extension().and_then(|ext| ext.to_str()) != Some(LOG_EXT) {
                        continue;
                    }
                    let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
                        continue;
                    };
                    out.push((stem.to_string(), path));
                }
            }
        }

        Ok(out)
    }

    /// 回收。**两个开关各自可选，默认都不动**（这是破坏性操作，宁可手动触发）：
    /// - `orphan_blobs`：没有任何日志引用的 blob（悬置的正文或补丁）；
    /// - `superseded_drafts`：夹在两次提交之间、已经被取代的草稿节点。
    ///
    /// 只回收 blob 是安全的：基准是用**版本号**引用的，不是哈希，所以不必理解增量链。
    pub fn gc(&self, orphan_blobs: bool, superseded_drafts: bool) -> Result<GcReport, VaultError> {
        let mut report = GcReport::default();
        let files = self.log_files()?;

        if superseded_drafts {
            for (_, path) in &files {
                let events = self.read_events_at(path)?;
                let superseded = fold(&events).superseded;
                if superseded.is_empty() {
                    continue;
                }

                let kept: Vec<&Event> = events
                    .iter()
                    .filter(|event| {
                        !matches!(event, Event::Auto { rev, .. } if superseded.contains(rev))
                    })
                    .collect();
                report.removed_drafts += events.len() - kept.len();

                let mut body = String::new();
                for event in kept {
                    body.push_str(&serde_json::to_string(event)?);
                    body.push('\n');
                }
                write_atomic(path, body.as_bytes())?;
            }
        }

        if orphan_blobs {
            // 清理过草稿之后要重新读一遍，引用集合才是准的
            let mut referenced: HashSet<String> = HashSet::new();
            for (_, path) in &files {
                for event in self.read_events_at(path)? {
                    match &event {
                        Event::Rev { blob, .. } | Event::Auto { blob, .. } => {
                            referenced.insert(blob.clone());
                        }
                        _ => {}
                    }
                }
            }

            let blobs_dir = self.root.join("blobs");
            let prefixes = match fs::read_dir(&blobs_dir) {
                Ok(entries) => entries,
                Err(_) => return Ok(report),
            };

            for prefix_entry in prefixes {
                let prefix_path = prefix_entry?.path();
                if !prefix_path.is_dir() {
                    continue;
                }

                for entry in fs::read_dir(&prefix_path)? {
                    let path = entry?.path();
                    let Some(hash) = path.file_name().and_then(|name| name.to_str()) else {
                        continue;
                    };

                    // 原子写留下的中间文件
                    if hash.ends_with(".tmp") {
                        let _ = fs::remove_file(&path);
                        continue;
                    }
                    if referenced.contains(hash) {
                        continue;
                    }

                    let size = fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
                    fs::remove_file(&path)?;
                    report.removed_blobs += 1;
                    report.freed_bytes += size;
                }
            }
        }

        Ok(report)
    }

    // ------------------------------------------------------------ 首次运行

    /// 仓库里一篇笔记都没有时，用给定内容建一篇（保证首次启动有东西可看）
    pub fn seed_if_empty(&self, title: &str, markdown_text: &str) -> Result<bool, VaultError> {
        if !self.walk(&self.notes_dir())?.is_empty() {
            return Ok(false);
        }
        self.create(title)?;
        self.commit(title, markdown_text, Some("初始化"), 0)?;
        Ok(true)
    }
}

// ---------------------------------------------------------------- 小工具

/// 默认仓库根：`~/.refind-note`
pub fn default_root() -> Result<PathBuf, VaultError> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .ok_or_else(|| {
            VaultError::Io(io::Error::new(
                io::ErrorKind::NotFound,
                "找不到用户主目录（HOME / USERPROFILE 都没有）",
            ))
        })?;
    Ok(PathBuf::from(home).join(DEFAULT_DIR_NAME))
}



/// 大小写不敏感地去掉 ASCII 前缀。
///
/// 刻意用 `get(..n)`：直接切 `&value[..n]` 会在多字节字符中间 panic。
fn strip_prefix_ci<'a>(value: &'a str, prefix: &str) -> Option<&'a str> {
    let head = value.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &value[prefix.len()..])
}

/// 规范地址拼装：`NAME[@STATE][#章节]`。
///
/// 命名空间（0 号＝主命名空间）与默认状态（看最新提交）都省略，所以最简单的笔记
/// 显示出来就是一个光秃秃的 `NAME`。
fn compose_address(title: &str, state: Option<&str>, section: Option<&str>) -> String {
    let mut out = title.to_string();
    if let Some(state) = state {
        out.push('@');
        out.push_str(state);
    }
    if let Some(section) = section {
        out.push('#');
        out.push_str(section);
    }
    out
}

fn now_iso() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}



#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    /// 测试用的临时仓库
    struct TempVault {
        vault: Vault,
        root: PathBuf,
    }

    impl TempVault {
        fn new() -> Self {
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let root =
                std::env::temp_dir().join(format!("refind-vault-{}-{n}", std::process::id()));
            let _ = fs::remove_dir_all(&root);
            let vault = Vault::open(&root).expect("open vault");
            Self { vault, root }
        }
    }

    impl Drop for TempVault {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn init_creates_expected_files() {
        let temp = TempVault::new();
        for dir in ["notes", "trash", "blobs"] {
            assert!(temp.root.join(dir).is_dir(), "{dir}/ 应当被建出来");
        }
        assert!(temp.root.join("vault.json").is_file());
        assert!(temp.root.join("namespaces.json").is_file());
        // 不再有全局索引：文件系统自己就是索引
        assert!(!temp.root.join("index.json").exists());
        assert!(temp.vault.list_notes().unwrap().is_empty());
    }

    #[test]
    fn commit_then_load_roundtrip() {
        let temp = TempVault::new();
        let note = temp.vault.create("测试条目").unwrap();
        assert_eq!(note.rev, 0);
        assert_eq!(note.key, "0:测试条目");

        let note = temp
            .vault
            .commit("测试条目", "# 标题\n\n正文", Some("初稿"), 0)
            .unwrap();
        assert_eq!(note.rev, 1);
        assert!(note.html.contains("<h1"), "{}", note.html);
        assert_eq!(temp.vault.load("测试条目").unwrap().rev, 1);
    }

    #[test]
    fn version_chain_keeps_parents_on_commits() {
        let temp = TempVault::new();
        temp.vault.create("链条").unwrap();
        temp.vault.commit("链条", "v1", None, 0).unwrap();
        temp.vault.commit("链条", "v2", None, 1).unwrap();
        temp.vault.commit("链条", "v3", None, 2).unwrap();
        assert_eq!(temp.vault.load("链条").unwrap().rev, 3);
        assert_eq!(temp.vault.load("链条").unwrap().markdown, "v3");
    }

    /// 文件名就是标题：列目录就能拿到全部笔记，不需要读文件、也不需要索引
    #[test]
    fn listing_comes_from_the_filesystem() {
        let temp = TempVault::new();
        temp.vault.create("甲").unwrap();
        temp.vault.create("乙").unwrap();
        temp.vault.commit("甲", "内容甲", None, 0).unwrap();

        let notes = temp.vault.list_notes().unwrap();
        assert_eq!(notes.len(), 2);

        // 改名之后列出来的必须是新名字（旧文件已经不在了）
        temp.vault.rename("甲", "丙").unwrap();
        let notes = temp.vault.list_notes().unwrap();
        let titles: Vec<&str> = notes.iter().map(|note| note.title.as_str()).collect();
        assert!(titles.contains(&"丙"), "{titles:?}");
        assert!(!titles.contains(&"甲"), "{titles:?}");
    }

    #[test]
    fn rename_moves_the_file_and_keeps_the_history() {
        let temp = TempVault::new();
        temp.vault.create("旧名").unwrap();
        temp.vault.commit("旧名", "正文", None, 0).unwrap();

        let (_, before) = temp.vault.locate("旧名").unwrap();
        let note = temp.vault.rename("旧名", "新名").unwrap();

        assert_eq!(note.key, "0:新名");
        assert_eq!(note.rev, 2, "改名本身是一次提交");
        assert_eq!(note.markdown, "正文", "内容不变");
        // 改名**不搬文件**：同一个 id、同一个路径
        assert!(before.is_file(), "改名不该搬文件");
        let (_, after) = temp.vault.locate("新名").unwrap();
        assert_eq!(after, before, "改名前后路径应当一样");
        // 历史跟着文件走
        assert_eq!(temp.vault.history("新名").unwrap().len(), 3);
        assert!(matches!(
            temp.vault.load("旧名"),
            Err(VaultError::NotFound(_))
        ));
    }

    #[test]
    fn delete_moves_the_file_to_trash() {
        let temp = TempVault::new();
        temp.vault.create("待删").unwrap();
        temp.vault.commit("待删", "正文", None, 0).unwrap();

        let (_, path) = temp.vault.locate("待删").unwrap();
        let id = Vault::id_from_path(&temp.vault.locate("待删").unwrap().1);
        temp.vault.delete("待删").unwrap();

        assert!(!path.is_file(), "笔记文件应当已经不在 notes/ 里");
        let trashed = temp.vault.trashed_path(&id);
        assert!(trashed.is_file(), "应当被挪进 trash/");

        // 删除标记还在：折叠出来是「已删除」
        let state = fold(&temp.vault.read_events_at(&trashed).unwrap());
        assert!(state.deleted);
        assert!(state.blob.is_some());

        // 而且能分辨「删过」和「从没建过」
        let outcome = temp.vault.load_outcome("待删").unwrap();
        assert!(outcome.note.is_none() && outcome.deleted);
        let outcome = temp.vault.load_outcome("从没建过").unwrap();
        assert!(outcome.note.is_none() && !outcome.deleted);
    }

    #[test]
    fn draft_is_chained_but_superseded_by_commit() {
        let temp = TempVault::new();
        temp.vault.create("草稿").unwrap();
        temp.vault.commit("草稿", "v1", None, 0).unwrap();

        temp.vault.save_draft("草稿", "v1-草稿a", 1).unwrap();
        temp.vault.save_draft("草稿", "v1-草稿b", 1).unwrap();

        let draft = temp.vault.load_draft("草稿").unwrap().unwrap();
        assert_eq!(draft.markdown, "v1-草稿b");
        assert_eq!(draft.base_rev, 1);

        let note = temp
            .vault
            .commit("草稿", "v1-草稿b", Some("提交"), 1)
            .unwrap();
        assert_eq!(note.rev, 4, "版本号是一条序列，草稿也占号");
        assert!(temp.vault.load_draft("草稿").unwrap().is_none());

        let events = temp.vault.events_for("草稿").unwrap();
        let commit = events
            .iter()
            .find_map(|event| match event {
                Event::Rev {
                    rev: 4,
                    supersedes,
                    parent,
                    ..
                } => Some((supersedes.clone(), *parent)),
                _ => None,
            })
            .expect("找到提交 4");
        assert_eq!(commit.0, vec![2, 3], "本次提交取代了那两个草稿节点");
        assert_eq!(commit.1, Some(1), "parent 必须指向上一个提交，而不是草稿");
    }

    #[test]
    fn prune_removes_superseded_drafts_without_breaking_the_chain() {
        let temp = TempVault::new();
        temp.vault.create("清理").unwrap();
        temp.vault.commit("清理", "v1", None, 0).unwrap();
        temp.vault.save_draft("清理", "draft-a", 1).unwrap();
        temp.vault.save_draft("清理", "draft-b", 1).unwrap();
        temp.vault.commit("清理", "v2", None, 1).unwrap();

        assert_eq!(temp.vault.prune("清理").unwrap(), 2);
        let note = temp.vault.load("清理").unwrap();
        assert_eq!(note.rev, 4);
        assert_eq!(note.markdown, "v2");
        assert!(!temp
            .vault
            .events_for("清理")
            .unwrap()
            .iter()
            .any(|event| matches!(event, Event::Auto { .. })));
    }

    #[test]
    fn discard_draft_removes_only_the_current_spur() {
        let temp = TempVault::new();
        temp.vault.create("丢弃").unwrap();
        temp.vault.commit("丢弃", "v1", None, 0).unwrap();
        temp.vault.save_draft("丢弃", "draft", 1).unwrap();

        assert_eq!(temp.vault.discard_draft("丢弃").unwrap(), 1);
        assert!(temp.vault.load_draft("丢弃").unwrap().is_none());
        assert_eq!(temp.vault.load("丢弃").unwrap().markdown, "v1");
    }

    #[test]
    fn identical_draft_is_not_appended_twice() {
        let temp = TempVault::new();
        temp.vault.create("幂等").unwrap();
        temp.vault.save_draft("幂等", "一样的内容", 0).unwrap();
        temp.vault.save_draft("幂等", "一样的内容", 0).unwrap();
        let autos = temp
            .vault
            .events_for("幂等")
            .unwrap()
            .iter()
            .filter(|event| matches!(event, Event::Auto { .. }))
            .count();
        assert_eq!(autos, 1, "内容没变不该重复追加");
    }

    #[test]
    fn commit_with_stale_base_rev_is_rejected() {
        let temp = TempVault::new();
        temp.vault.create("冲突").unwrap();
        temp.vault.commit("冲突", "v1", None, 0).unwrap();
        assert!(matches!(
            temp.vault.commit("冲突", "v2", None, 0).unwrap_err(),
            VaultError::Conflict {
                expected: 0,
                found: 1
            }
        ));
    }

    #[test]
    fn renaming_onto_an_existing_title_is_rejected() {
        let temp = TempVault::new();
        temp.vault.create("甲").unwrap();
        temp.vault.create("乙").unwrap();
        assert!(matches!(
            temp.vault.rename("甲", "乙").unwrap_err(),
            VaultError::NameTaken(_)
        ));
    }

    #[test]
    fn red_and_blue_links_are_marked() {
        let temp = TempVault::new();
        temp.vault.create("目标条目").unwrap();
        temp.vault.commit("目标条目", "我是目标", None, 0).unwrap();
        temp.vault.create("来源").unwrap();
        let note = temp
            .vault
            .commit("来源", "[[目标条目|去看]] 和 [[不存在的条目]]", None, 0)
            .unwrap();
        assert!(note.html.contains(r#"data-key="0:目标条目""#), "{}", note.html);
        assert!(note.html.contains(r#"data-missing="false""#), "{}", note.html);
        assert!(note.html.contains(r#"data-missing="true""#), "{}", note.html);
    }

    #[test]
    fn seed_runs_only_once() {
        let temp = TempVault::new();
        assert!(temp.vault.seed_if_empty("示例", "内容").unwrap());
        assert!(!temp.vault.seed_if_empty("示例", "内容").unwrap());
        assert_eq!(temp.vault.list_notes().unwrap().len(), 1);
    }

    #[test]
    fn log_format_is_stable() {
        let temp = TempVault::new();
        temp.vault.seed_if_empty("格式", "内容").unwrap();
        temp.vault.save_draft("格式", "草稿", 1).unwrap();
        temp.vault.commit("格式", "定稿", Some("提交"), 1).unwrap();

        let id = temp.vault.note_ids().unwrap().remove(0);
        let log = temp
            .vault
            .read_events_at(&temp.vault.log_path(&id))
            .map(|events| {
                events
                    .iter()
                    .map(|event| serde_json::to_string(event).unwrap())
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap();

        println!("---- notes/{id}.log ----\n{log}");
        let lines: Vec<&str> = log.lines().collect();
        assert_eq!(lines.len(), 4, "meta + rev1 + auto + rev2");
        assert!(lines[0].contains(r#""t":"meta""#), "{}", lines[0]);
        assert!(lines[1].contains(r#""t":"rev""#), "{}", lines[1]);
        assert!(lines[2].contains(r#""t":"auto""#), "{}", lines[2]);
        assert!(lines[2].contains(r#""on":1"#), "{}", lines[2]);
        assert!(lines[3].contains(r#""supersedes":[2]"#), "{}", lines[3]);
        assert!(lines[3].contains(r#""parent":1"#), "{}", lines[3]);
    }

    #[test]
    fn opens_and_creates_a_missing_vault() {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let base = std::env::temp_dir().join(format!("refind-make-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let root = base.join("nested").join("deeper");

        assert!(!root.exists(), "前提：这个目录一开始不存在");
        let vault = Vault::open(&root).expect("应当自动建库");
        for dir in ["notes", "trash", "blobs"] {
            assert!(root.join(dir).is_dir(), "{dir}/ 应当被建出来");
        }
        assert!(root.join("vault.json").is_file());
        assert!(root.join("namespaces.json").is_file());
        assert!(vault.list_notes().unwrap().is_empty());

        println!("---- 自动建出来的仓库：{} ----", root.display());
        let mut entries: Vec<_> = fs::read_dir(&root)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        entries.sort();
        for path in entries {
            let kind = if path.is_dir() { "目录" } else { "文件" };
            println!("  {kind}  {}", path.file_name().unwrap().to_string_lossy());
        }

        let _ = fs::remove_dir_all(&base);
    }

    fn count_blobs(root: &std::path::Path) -> usize {
        let mut count = 0;
        if let Ok(prefixes) = fs::read_dir(root.join("blobs")) {
            for prefix in prefixes.flatten() {
                if let Ok(entries) = fs::read_dir(prefix.path()) {
                    count += entries.flatten().count();
                }
            }
        }
        count
    }

    /// GC 的两个开关各自可控，而且只回收该回收的
    #[test]
    fn gc_switches_are_independent() {
        let temp = TempVault::new();
        temp.vault.create("回收").unwrap();
        temp.vault.commit("回收", "v1", None, 0).unwrap();
        temp.vault.save_draft("回收", "草稿甲", 1).unwrap();
        temp.vault.commit("回收", "v2", None, 1).unwrap();

        // 只开草稿那一项：blob 一个都不该动
        let before = count_blobs(&temp.root);
        let report = temp.vault.gc(false, true).unwrap();
        assert_eq!(report.removed_drafts, 1, "那条草稿已被提交取代");
        assert_eq!(report.removed_blobs, 0, "没开就不动 blob");
        assert_eq!(report.freed_bytes, 0);
        assert_eq!(count_blobs(&temp.root), before);

        // 再开 blob 那一项：草稿的正文已无引用，应当被回收
        let report = temp.vault.gc(true, false).unwrap();
        assert!(report.removed_blobs >= 1, "草稿正文已经没人引用了");
        assert!(report.freed_bytes > 0);
        assert_eq!(report.removed_drafts, 0, "没开就不动草稿");

        // 回收之后内容照样读得出来
        assert_eq!(temp.vault.load("回收").unwrap().markdown, "v2");
    }

    /// 被删除的笔记也要参与引用统计，否则它的正文会被误回收
    #[test]
    fn gc_keeps_blobs_referenced_by_trashed_notes() {
        let temp = TempVault::new();
        temp.vault.create("待删").unwrap();
        temp.vault.commit("待删", "要被保留的正文", None, 0).unwrap();
        temp.vault.delete("待删").unwrap();

        let report = temp.vault.gc(true, true).unwrap();
        assert_eq!(report.removed_blobs, 0, "trash 里的笔记还在引用它");

        let id = Vault::id_from_path(&temp.vault.locate("待删").unwrap().1);
        let state = fold(&temp.vault.read_events_at(&temp.vault.trashed_path(&id)).unwrap());
        let blob = state.blob.expect("应当还有内容引用");
        assert_eq!(
            String::from_utf8(temp.vault.blobs.get(&blob).unwrap()).unwrap(),
            "要被保留的正文"
        );
    }

    #[test]
    fn validate_title_reports_the_same_rules_as_parsing() {
        let temp = TempVault::new();
        assert!(temp.vault.validate_title("合法标题").is_ok());
        assert!(temp.vault.validate_title("带@符号").is_err());
        assert!(temp.vault.validate_title("Help:目录").is_err(), "只有主命名空间");
        assert!(temp.vault.validate_title("").is_err());
        // 校验不该留下任何文件
        assert!(temp.root.join("notes").read_dir().unwrap().next().is_none());
    }

    /// 每个版本都有稳定 ID，数字与缩写两种引用都能解析
    #[test]
    fn revisions_have_short_ids_that_resolve() {
        let temp = TempVault::new();
        temp.vault.create("编号").unwrap();
        temp.vault.commit("编号", "第一版", None, 0).unwrap();
        temp.vault.commit("编号", "第二版", None, 1).unwrap();

        let history = temp.vault.history("编号").unwrap();
        let second = history.iter().find(|item| item.rev == 2).unwrap();
        assert_eq!(second.id.len(), 64, "完整 ID 是 sha256");
        assert_eq!(second.short_id.len(), 8);
        assert!(second.id.starts_with(&second.short_id));

        // ID 必须稳定：再查一次还是同一个
        let again = temp.vault.history("编号").unwrap();
        assert_eq!(
            again.iter().find(|item| item.rev == 2).unwrap().id,
            second.id
        );

        // 数字与缩写都能解析到同一个版本
        assert_eq!(temp.vault.resolve_revision("编号", "2").unwrap(), 2);
        assert_eq!(
            temp.vault.resolve_revision("编号", &second.short_id).unwrap(),
            2
        );
        assert_eq!(temp.vault.resolve_revision("编号", &second.id).unwrap(), 2);

        // 对不上的缩写要给「找不到」，而不是猜一个
        assert!(matches!(
            temp.vault.resolve_revision("编号", "zzzzzzzz"),
            Err(VaultError::RevisionNotFound { .. })
        ));
    }

    /// 地址栏解析：标题、标题@数字、标题@缩写、只写 @缩写、不存在、歧义


    /// 缩写下限：太短要明确报错（纯数字版本号不受限）
    #[test]
    fn short_ids_have_a_minimum_length() {
        let temp = TempVault::new();
        temp.vault.create("下限").unwrap();
        temp.vault.commit("下限", "第一版", None, 0).unwrap();

        assert_eq!(temp.vault.resolve_revision("下限", "1").unwrap(), 1, "数字版本号不受限");

        let error = temp.vault.resolve_revision("下限", "ab").unwrap_err();
        assert!(
            matches!(error, VaultError::BadAddress(_)),
            "太短的缩写应当是提示，而不是「找不到」：{error}"
        );

        let history = temp.vault.history("下限").unwrap();
        let short = history.iter().find(|item| item.rev == 1).unwrap().short_id.clone();
        assert!(short.len() >= 5, "展示用的缩写本来就有 8 位");
        assert_eq!(temp.vault.resolve_revision("下限", &short).unwrap(), 1);
    }

    /// 草稿也带 ID，界面才能用「标题@缩写」预览它
    #[test]
    fn drafts_expose_their_own_id() {
        let temp = TempVault::new();
        temp.vault.create("草稿号").unwrap();
        temp.vault.commit("草稿号", "第一版", None, 0).unwrap();
        temp.vault.save_draft("草稿号", "草稿内容", 1).unwrap();

        let draft = temp.vault.load_draft("草稿号").unwrap().unwrap();
        assert_eq!(draft.id.len(), 64);
        assert!(draft.short_id.len() >= 5);

        // 用这个缩写能解析到草稿那一版，并读到它的内容
        let rev = temp.vault.resolve_revision("草稿号", &draft.short_id).unwrap();
        assert_eq!(rev, 2);
        assert_eq!(temp.vault.revision("草稿号", rev).unwrap().markdown, "草稿内容");
    }

    /// 回归：8 位十六进制缩写里约 2% 会**恰好全是数字**，
    /// 以前先按「纯数字」判断，会把这种缩写误当版本号（偶发把 rev 解析成八位数）。
    #[test]
    fn all_digit_short_ids_still_resolve() {
        let temp = TempVault::new();
        temp.vault.create("回归").unwrap();

        let mut text = String::from("起始\n");
        temp.vault.commit("回归", &text, None, 0).unwrap();
        for round in 0..200 {
            text.push_str(&format!("第 {round} 行\n"));
            temp.vault
                .commit("回归", &text, None, (round + 1) as u64)
                .unwrap();
        }

        let history = temp.vault.history("回归").unwrap();
        assert_eq!(history.len(), 202, "1 条创建 + 201 次提交");

        // 每一版的缩写都必须解析回**它自己**（全数字的那些也一样）
        let mut all_digit = 0;
        for item in history.iter().filter(|item| item.rev > 0) {
            if item.short_id.chars().all(|ch| ch.is_ascii_digit()) {
                all_digit += 1;
            }
            assert_eq!(
                temp.vault.resolve_revision("回归", &item.short_id).unwrap(),
                item.rev,
                "缩写 {} 应当解析回版本 {}",
                item.short_id,
                item.rev
            );
        }
        println!("200 个缩写里有 {all_digit} 个恰好全是数字");
    }

    /// 碰撞防线：宁可拒绝写入，也不要留下两个同一个 ID 的版本
    #[test]
    fn id_collisions_are_rejected() {
        let temp = TempVault::new();
        temp.vault.create("撞车").unwrap();
        temp.vault.commit("撞车", "正文", None, 0).unwrap();

        let (_parsed, _) = temp.vault.locate("撞车").unwrap();
        let id = Vault::id_from_path(&temp.vault.locate("撞车").unwrap().1);
        let events = temp.vault.events_for("撞车").unwrap();
        let duplicate = events
            .iter()
            .find(|event| matches!(event, Event::Rev { .. }))
            .expect("有提交事件")
            .clone();

        // 把同一个事件再写一次：派生出的 ID 必然相同
        let error = temp.vault.append(&id, &duplicate).unwrap_err();
        assert!(
            matches!(error, VaultError::IdCollision(_)),
            "重复 ID 应当被拒绝：{error}"
        );

        // 拒绝之后日志没有被改动
        assert_eq!(
            temp.vault.read_events(&id).unwrap().len(),
            events.len(),
            "拒绝写入时不该落下任何东西"
        );
    }

    /// 模式也是地址语法的一部分，并且带回规范地址用于回显


    /// 标准顺序 NAMESPACE:NAME@VERSION#SECTION$STATE：顺序不强制，回显一律标准


    /// 规范化裁剪：某一版不能编辑，历史不属于某一版


    /// 新语法：状态用 @，版本用 view- / rollback- 前缀（版本收起来，不再组合）


    /// 页面地址模型：NAMESPACE:NAME@STATE，默认折叠成最简的 NAME
    #[test]
    fn address_model_collapses_to_the_simplest_form() {
        let temp = TempVault::new();
        temp.vault.create("模型").unwrap();
        temp.vault.commit("模型", "第一版", None, 0).unwrap();
        temp.vault.commit("模型", "第二版", None, 1).unwrap();

        let first = temp
            .vault
            .history("模型")
            .unwrap()
            .into_iter()
            .find(|item| item.rev == 1)
            .unwrap();

        // 默认（命名空间 0 + 看最新提交）→ 最简形式
        match temp.vault.parse_address("模型").unwrap() {
            Address::Note { title, address } => {
                assert_eq!(title, "模型");
                assert_eq!(address, "模型");
            }
            other => panic!("{other:?}"),
        }

        // 其余状态
        match temp.vault.parse_address("模型@edit").unwrap() {
            Address::Edit { address, .. } => assert_eq!(address, "模型@edit"),
            other => panic!("{other:?}"),
        }
        match temp.vault.parse_address("模型@history").unwrap() {
            Address::History { address, .. } => assert_eq!(address, "模型@history"),
            other => panic!("{other:?}"),
        }
        match temp.vault.parse_address("模型@delete").unwrap() {
            Address::Delete { address, .. } => assert_eq!(address, "模型@delete"),
            other => panic!("{other:?}"),
        }

        // 状态与章节一起
        match temp.vault.parse_address("模型@history#小节").unwrap() {
            Address::History { address, .. } => assert_eq!(address, "模型@history#小节"),
            other => panic!("{other:?}"),
        }

        // view-版本：缩写与数字版本号都行，**回显一律缩写 + 完整状态**
        match temp
            .vault
            .parse_address(&format!("模型@view-{}", first.short_id))
            .unwrap()
        {
            Address::ViewVersion {
                title,
                rev,
                short_id,
                address,
                ..
            } => {
                assert_eq!(title, "模型");
                assert_eq!(rev, 1);
                assert_eq!(short_id, first.short_id);
                assert_eq!(address, format!("模型@view-{}", first.short_id));
            }
            other => panic!("{other:?}"),
        }
        match temp.vault.parse_address("模型@view-1").unwrap() {
            Address::ViewVersion { rev, address, .. } => {
                assert_eq!(rev, 1);
                assert_eq!(address, format!("模型@view-{}", first.short_id));
            }
            other => panic!("{other:?}"),
        }

        // rollback-版本
        match temp.vault.parse_address("模型@rollback-1").unwrap() {
            Address::RollbackConfirm { rev, address, .. } => {
                assert_eq!(rev, 1);
                assert_eq!(address, format!("模型@rollback-{}", first.short_id));
            }
            other => panic!("{other:?}"),
        }

        // 状态写错要报错；回退不指出哪一版也报错
        assert!(temp.vault.parse_address("模型@whatever").is_err());
        assert!(temp.vault.parse_address("模型@rollback").is_err());
        // 版本对不上是错误
        assert!(temp.vault.parse_address("模型@view-abcde").is_err());
    }

    /// 端到端：界面按地址驱动的三条流程 —— 看某一版、回退确认、删除确认
    #[test]
    fn address_driven_flows_end_to_end() {
        let temp = TempVault::new();
        temp.vault.create("流程").unwrap();
        temp.vault.commit("流程", "第一版", None, 0).unwrap();
        temp.vault.commit("流程", "第二版", None, 1).unwrap();

        // 看某一版：地址给出 rev，界面据此取内容
        let rev = match temp.vault.parse_address("流程@view-1").unwrap() {
            Address::ViewVersion { rev, .. } => rev,
            other => panic!("{other:?}"),
        };
        assert_eq!(rev, 1);
        assert_eq!(temp.vault.revision("流程", rev).unwrap().markdown, "第一版");

        // 回退确认页 → 用户确认 → 真的回退（界面调 revert_note，这里是它的等价动作）
        let (title, rev) = match temp.vault.parse_address("流程@rollback-1").unwrap() {
            Address::RollbackConfirm { title, rev, .. } => (title, rev),
            other => panic!("{other:?}"),
        };
        let old = temp.vault.revision(&title, rev).unwrap().markdown;
        temp.vault.commit(&title, &old, Some("回退"), 2).unwrap();
        assert_eq!(temp.vault.load("流程").unwrap().markdown, "第一版");
        assert_eq!(temp.vault.load("流程").unwrap().rev, 3, "回退是一次新提交");

        // 删除确认页 → 用户确认 → 文件进 trash/，再解析同一地址就是「不存在」
        match temp.vault.parse_address("流程@delete").unwrap() {
            Address::Delete { title, .. } => temp.vault.delete(&title).unwrap(),
            other => panic!("{other:?}"),
        }
        match temp.vault.parse_address("流程@delete").unwrap() {
            Address::Missing { title, .. } => assert_eq!(title, "流程"),
            other => panic!("{other:?}"),
        }
    }

    /// 虚拟命名空间 special:：不对应笔记，交给前端渲染；大小写不敏感
    #[test]
    fn special_namespace_does_not_map_to_a_note() {
        let temp = TempVault::new();

        match temp.vault.parse_address("special:newtab").unwrap() {
            Address::Special { page, address } => {
                assert_eq!(page, "newtab");
                assert_eq!(address, "special:newtab");
            }
            other => panic!("{other:?}"),
        }

        // 大小写不敏感，回显一律小写
        match temp.vault.parse_address("Special:NewTab").unwrap() {
            Address::Special { page, address } => {
                assert_eq!(page, "newtab");
                assert_eq!(address, "special:newtab");
            }
            other => panic!("{other:?}"),
        }

        // 章节跟着特殊页面走（前端自己处理），不进 page 名
        match temp.vault.parse_address("special:newtab#小节").unwrap() {
            Address::Special { page, .. } => assert_eq!(page, "newtab"),
            other => panic!("{other:?}"),
        }

        // 空页面名要报错
        assert!(temp.vault.parse_address("special:").is_err());

        // 笔记不可能叫这个名字：标题里禁止冒号
        assert!(temp.vault.validate_title("special:newtab").is_err());
    }

    /// 回归：多字节标题不能让解析 panic。
    ///
    /// 曾经写成 `raw[..8]`（按字节切），中文标题会让切口落在字符中间 —— 而且这个
    /// panic 在 GTK 回调里不能 unwind，会把整个应用 abort。
    #[test]
    fn multi_byte_titles_do_not_panic() {
        let temp = TempVault::new();

        // 「平陆运河」是 12 字节，第 8 个字节落在「运」中间
        assert!(matches!(
            temp.vault.parse_address("平陆运河").unwrap(),
            Address::Missing { .. }
        ));

        // 各种长度都过一遍，确保没有别处按字节切
        for title in ["页", "页面", "页面名", "页面名字", "页面名字啊", "页面名字啊啊"] {
            let _ = temp.vault.parse_address(title);
            let _ = temp.vault.validate_title(title);
        }

        // 大小写不敏感的前缀判断本身也要能处理多字节
        assert!(strip_prefix_ci("Special:newtab", "special:").is_some());
        assert!(strip_prefix_ci("特殊:newtab", "special:").is_none());
    }

    /// 数据模型：**文件名是纯 ASCII id，标题只存在 titles.json 里**
    #[test]
    fn file_names_are_hex_ids_and_titles_live_in_json() {
        let temp = TempVault::new();
        temp.vault.create("标题不进文件名").unwrap();
        temp.vault
            .commit("标题不进文件名", "正文", None, 0)
            .unwrap();

        let ids = temp.vault.note_ids().unwrap();
        assert_eq!(ids.len(), 1);
        let id = ids[0].clone();
        assert!(
            id.chars().all(|ch| ch.is_ascii_hexdigit()),
            "文件名应当是纯十六进制：{id}"
        );
        assert!(
            temp.root
                .join("notes")
                .join("0")
                .join(format!("{id}.log"))
                .is_file(),
            "内容应当在 notes/0/<id>.log"
        );

        let table = fs::read_to_string(temp.root.join("titles.json")).unwrap();
        assert!(
            table.contains("标题不进文件名"),
            "名字应当存在 titles.json 里：{table}"
        );

        // 改名：文件跟着 id 走（换到新标题的 id），名字表里换一行，旧名字不留
        temp.vault.rename("标题不进文件名", "换个名字").unwrap();
        assert_eq!(temp.vault.note_ids().unwrap().len(), 1, "改名不该留下旧文件");
        let table = fs::read_to_string(temp.root.join("titles.json")).unwrap();
        assert!(table.contains("换个名字"), "{table}");
        assert!(!table.contains("标题不进文件名"), "{table}");

        // 删除：名字从 notes 挪到 trashed，文件进 trash/
        temp.vault.delete("换个名字").unwrap();
        let table = fs::read_to_string(temp.root.join("titles.json")).unwrap();
        assert!(table.contains("trashed"), "{table}");
        assert!(table.contains("换个名字"), "删除后名字要留在 trashed 里：{table}");
    }

    /// 状态与章节随便怎么排，回显一律标准顺序；看最新提交时 `view-` 会被裁掉
    #[test]
    fn address_is_normalized_and_latest_view_is_cropped() {
        let temp = TempVault::new();
        temp.vault.create("顺序").unwrap();
        temp.vault.commit("顺序", "第一版", None, 0).unwrap();
        temp.vault.commit("顺序", "第二版", None, 1).unwrap();

        let history = temp.vault.history("顺序").unwrap();
        let first = history.iter().find(|item| item.rev == 1).unwrap();
        let latest = history.iter().find(|item| item.rev == 2).unwrap();

        // 顺序乱写（章节在前、状态在后）→ 回显按 `NAME@STATE#SECTION`
        match temp
            .vault
            .parse_address(&format!("顺序#小节@view-{}", first.short_id))
            .unwrap()
        {
            Address::ViewVersion { address, rev, .. } => {
                assert_eq!(rev, 1);
                assert_eq!(address, format!("顺序@view-{}#小节", first.short_id));
            }
            other => panic!("{other:?}"),
        }

        // 看的就是最新提交 → 裁掉 `view-`
        match temp
            .vault
            .parse_address(&format!("顺序@view-{}", latest.short_id))
            .unwrap()
        {
            Address::Note { address, title } => {
                assert_eq!(title, "顺序");
                assert_eq!(address, "顺序");
            }
            other => panic!("{other:?}"),
        }

        // 带章节时：裁状态、留章节
        match temp
            .vault
            .parse_address(&format!("顺序@view-{}#小节", latest.short_id))
            .unwrap()
        {
            Address::Note { address, .. } => assert_eq!(address, "顺序#小节"),
            other => panic!("{other:?}"),
        }

        // rollback 不是默认状态，不能裁
        match temp
            .vault
            .parse_address(&format!("顺序@rollback-{}", latest.short_id))
            .unwrap()
        {
            Address::RollbackConfirm { address, .. } => {
                assert_eq!(address, format!("顺序@rollback-{}", latest.short_id));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn default_root_is_under_home() {
        let root = default_root().expect("应当能定位到主目录");
        assert_eq!(
            root.file_name().and_then(|name| name.to_str()),
            Some(DEFAULT_DIR_NAME)
        );
        if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
            assert!(root.starts_with(&home), "{} 应当在家目录下", root.display());
        }
    }

    #[test]
    fn history_lists_commits_and_drafts() {
        let temp = TempVault::new();
        temp.vault.create("历史").unwrap();
        temp.vault.commit("历史", "第一版", Some("初稿"), 0).unwrap();
        temp.vault.save_draft("历史", "第一版又加了一点", 1).unwrap();
        temp.vault.commit("历史", "第二版", Some("定稿"), 1).unwrap();

        let history = temp.vault.history("历史").unwrap();
        let kinds: Vec<&str> = history.iter().map(|item| item.kind.as_str()).collect();
        assert_eq!(kinds, vec!["create", "commit", "draft", "commit"]);
        assert_eq!(history[1].summary.as_deref(), Some("初稿"));
        assert_eq!(history[3].rev, 3);
        assert_eq!(history[3].supersedes, vec![2]);
        assert_eq!(history[1].bytes, 9, "「第一版」是 3 个 CJK 字");
        assert_eq!(history[1].delta, 9);
    }

    #[test]
    fn revision_reads_any_version_including_drafts() {
        let temp = TempVault::new();
        temp.vault.create("版本").unwrap();
        temp.vault.commit("版本", "第一版", None, 0).unwrap();
        temp.vault.save_draft("版本", "草稿内容", 1).unwrap();

        let first = temp.vault.revision("版本", 1).unwrap();
        assert_eq!(first.kind, "commit");
        assert_eq!(first.markdown, "第一版");
        assert!(first.html.contains("第一版"), "{}", first.html);

        let draft = temp.vault.revision("版本", 2).unwrap();
        assert_eq!(draft.kind, "draft");
        assert_eq!(draft.markdown, "草稿内容");

        assert!(matches!(
            temp.vault.revision("版本", 99),
            Err(VaultError::RevisionNotFound { rev: 99, .. })
        ));
    }

    #[test]
    fn compare_reports_changes_between_versions() {
        let temp = TempVault::new();
        temp.vault.create("对比").unwrap();
        temp.vault.commit("对比", "一\n二\n三", None, 0).unwrap();
        temp.vault.commit("对比", "一\n改\n三", None, 1).unwrap();

        let result = temp.vault.compare("对比", 1, 2).unwrap();
        assert_eq!(result.from_rev, 1);
        assert_eq!(result.to_rev, 2);
        assert_eq!(result.inserted, 1);
        assert_eq!(result.deleted, 1);
        assert!(result
            .lines
            .iter()
            .any(|line| line.kind == "insert" && line.text == "改"));
    }

    /// 小改动应当真的用上增量，而且回放必须逐字节还原
    #[test]
    fn small_edits_are_stored_as_deltas() {
        let temp = TempVault::new();
        temp.vault.create("增量").unwrap();
        let first = "一\n二\n三\n四\n五\n六\n七\n八\n九\n十\n";
        temp.vault.commit("增量", first, None, 0).unwrap();

        let second = first.replace("五", "改过的五");
        let note = temp.vault.commit("增量", &second, None, 1).unwrap();
        assert_eq!(note.markdown, second, "增量回放必须逐字节还原");

        let events = temp.vault.events_for("增量").unwrap();
        let encodings: Vec<String> = events
            .iter()
            .filter_map(|event| match event {
                Event::Rev { encoding, .. } => Some(encoding.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(encodings, vec!["full", "delta"], "第二版应当用增量");

        let history = temp.vault.history("增量").unwrap();
        assert_eq!(history[2].encoding, "delta");
        assert_eq!(
            history[2].bytes as usize,
            second.len(),
            "bytes 记的是完整内容的大小，不是补丁大小"
        );
    }

    /// 大改动时补丁反而更大，必须退回整份快照（规则一）
    #[test]
    fn heavy_rewrites_fall_back_to_full_snapshots() {
        let temp = TempVault::new();
        temp.vault.create("重写").unwrap();
        temp.vault.commit("重写", "甲\n", None, 0).unwrap();

        let mut target = String::new();
        for index in 0..200 {
            target.push_str(&format!("全新的第 {index} 行内容\n"));
        }
        let note = temp.vault.commit("重写", &target, None, 1).unwrap();
        assert_eq!(note.markdown, target);

        let encoding = temp
            .vault
            .events_for("重写")
            .unwrap()
            .iter()
            .find_map(|event| match event {
                Event::Rev { rev: 2, encoding, .. } => Some(encoding.clone()),
                _ => None,
            })
            .unwrap();
        assert_eq!(encoding, "full", "大改动应当存整份");
    }

    /// 增量链到上限后必须再落一次整份快照，且每版都还要能还原（规则二）
    #[test]
    fn chain_length_is_capped() {
        let temp = TempVault::new();
        temp.vault.create("链长").unwrap();

        let mut text = String::from("第一版\n");
        temp.vault.commit("链长", &text, None, 0).unwrap();
        for round in 0..34 {
            text.push_str(&format!("第 {round} 次追加\n"));
            temp.vault
                .commit("链长", &text, None, (round + 1) as u64)
                .unwrap();
        }

        let encodings: Vec<String> = temp
            .vault
            .events_for("链长")
            .unwrap()
            .iter()
            .filter_map(|event| match event {
                Event::Rev { encoding, .. } => Some(encoding.clone()),
                _ => None,
            })
            .collect();
        let fulls = encodings.iter().filter(|item| item.as_str() == "full").count();
        assert!(fulls >= 2, "链到上限后应当再落一次整份快照：{encodings:?}");

        // 最新一版与中间某一版都要能正确回放
        assert_eq!(temp.vault.load("链长").unwrap().markdown, text);
        let mut fifth = String::from("第一版\n");
        for round in 0..4 {
            fifth.push_str(&format!("第 {round} 次追加\n"));
        }
        assert_eq!(temp.vault.revision("链长", 5).unwrap().markdown, fifth);
    }

    /// 提交的基准只能是提交，绝不能是草稿（规则三）
    #[test]
    fn commits_never_depend_on_drafts() {
        let temp = TempVault::new();
        temp.vault.create("依赖").unwrap();
        temp.vault.commit("依赖", "v1", None, 0).unwrap();
        temp.vault.save_draft("依赖", "草稿", 1).unwrap();
        temp.vault.commit("依赖", "v2", None, 1).unwrap();

        let bases: Vec<Option<u64>> = temp
            .vault
            .events_for("依赖")
            .unwrap()
            .iter()
            .filter_map(|event| match event {
                Event::Rev { base_rev, .. } => Some(*base_rev),
                _ => None,
            })
            .collect();
        assert!(
            bases.iter().all(|base| *base != Some(2)),
            "提交的基准不能指向草稿（版本 2）：{bases:?}"
        );

        // 草稿被清理之后内容仍然读得出来 —— 这正是这条规则要保证的
        temp.vault.prune("依赖").unwrap();
        assert_eq!(temp.vault.load("依赖").unwrap().markdown, "v2");
    }

    #[test]
    fn a_renamed_version_keeps_the_title_of_its_time() {
        let temp = TempVault::new();
        temp.vault.create("旧标题").unwrap();
        temp.vault.commit("旧标题", "正文", None, 0).unwrap();
        temp.vault.rename("旧标题", "新标题").unwrap();

        let old = temp.vault.revision("新标题", 1).unwrap();
        assert_eq!(old.title, "旧标题", "版本 1 当时的标题还是旧的");
        let new = temp.vault.revision("新标题", 2).unwrap();
        assert_eq!(new.title, "新标题");
        assert_eq!(new.markdown, "正文");
    }
}
