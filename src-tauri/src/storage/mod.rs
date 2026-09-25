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
mod index;

pub use api::{
    DiffResult, Draft, LoadOutcome, Note, NoteSummary, RevisionContent, RevisionSummary,
    VaultSettings,
};
pub use config::VaultConfig;
pub use error::VaultError;
pub use event::{fold, next_rev, Event};
pub use index::{IndexEntry, IndexFile};

use atomic::{append_line, hash_bytes, write_atomic, BlobStore};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// 仓库目录名（放在用户主目录下）
pub const DEFAULT_DIR_NAME: &str = ".refind-note";
pub const DEFAULT_MIME: &str = "text/markdown";

// ---------------------------------------------------------------- 仓库

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

    // ------------------------------------------------------------ 路径

    fn notes_dir(&self) -> PathBuf {
        self.root.join("notes")
    }

    fn log_path(&self, id: &str) -> PathBuf {
        self.notes_dir().join(format!("{id}.log"))
    }

    fn index_path(&self) -> PathBuf {
        self.root.join("index.json")
    }

    // ------------------------------------------------------------ id

    /// `m<时间戳 base36>-<纳秒 base36>`：按创建时间可排序、无依赖。
    /// 同毫秒同纳秒不现实，但仍做一次存在性检查兜底。
    fn new_id(&self) -> Result<String, VaultError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let base = format!(
            "m{}-{}",
            to_base36(now.as_millis() as u64),
            to_base36(now.subsec_nanos() as u64)
        );
        let mut candidate = base.clone();
        let mut n = 0u32;
        while self.log_path(&candidate).exists() {
            n += 1;
            candidate = format!("{base}-{n}");
        }
        Ok(candidate)
    }

    // ------------------------------------------------------------ 日志

    fn read_events(&self, id: &str) -> Result<Vec<Event>, VaultError> {
        let path = self.log_path(id);
        let text = match fs::read_to_string(&path) {
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

    fn append(&self, id: &str, event: &Event) -> Result<(), VaultError> {
        let line = serde_json::to_string(event)?;
        append_line(&self.log_path(id), &line)?;
        Ok(())
    }

    /// 重写日志（只用于 prune / 丢弃草稿这类显式维护操作）
    fn rewrite_log(&self, id: &str, keep: impl Fn(&Event) -> bool) -> Result<usize, VaultError> {
        let events = self.read_events(id)?;
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
        write_atomic(&self.log_path(id), body.as_bytes())?;
        Ok(removed)
    }

    fn note_ids(&self) -> Result<Vec<String>, VaultError> {
        let mut ids = Vec::new();
        let entries = match fs::read_dir(self.notes_dir()) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(ids),
            Err(error) => return Err(error.into()),
        };
        for entry in entries {
            let path = entry?.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("log") {
                continue;
            }
            if let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) {
                ids.push(stem.to_string());
            }
        }
        Ok(ids)
    }

    // ------------------------------------------------------------ 索引（只是缓存）

    fn load_index(&self) -> Result<HashMap<String, IndexEntry>, VaultError> {
        match fs::read_to_string(self.index_path()) {
            Ok(text) => match serde_json::from_str::<IndexFile>(&text) {
                Ok(file) => Ok(file.notes),
                // 缓存坏了就当没有，重建即可
                Err(error) => {
                    eprintln!("[vault] index.json 无法解析，重建：{error}");
                    self.rebuild_index()
                }
            },
            Err(_) => self.rebuild_index(),
        }
    }

    fn save_index(&self, notes: &HashMap<String, IndexEntry>) -> Result<(), VaultError> {
        let file = IndexFile {
            format: 1,
            built_at: now_iso(),
            notes: notes.clone(),
        };
        write_atomic(&self.index_path(), &serde_json::to_vec_pretty(&file)?)?;
        Ok(())
    }

    /// 扫描全部日志重建索引（也是修复手段，以及「从日志完全重放」的入口）
    pub fn rebuild_index(&self) -> Result<HashMap<String, IndexEntry>, VaultError> {
        let mut notes = HashMap::new();
        for id in self.note_ids()? {
            let events = self.read_events(&id)?;
            let state = fold(&events);
            if state.title.is_empty() || state.deleted {
                continue;
            }
            notes.insert(
                state.key(),
                IndexEntry {
                    id,
                    ns: state.ns,
                    title: state.title.clone(),
                    rev: state.rev,
                    at: state.at.clone(),
                },
            );
        }
        self.save_index(&notes)?;
        Ok(notes)
    }

    fn find(&self, title: &str) -> Result<(ParsedTitle, IndexEntry), VaultError> {
        let parsed = self.table.parse(title, self.config.capital_links)?;
        let index = self.load_index()?;
        match index.get(&parsed.key()) {
            Some(entry) => Ok((parsed, entry.clone())),
            None => Err(VaultError::NotFound(parsed.display(&self.table))),
        }
    }

    // ------------------------------------------------------------ 链接解析

    fn resolver(
        &self,
        index: &HashMap<String, IndexEntry>,
        from: Option<ParsedTitle>,
    ) -> LinkResolver {
        let keys: HashSet<String> = index.keys().cloned().collect();
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

    // ------------------------------------------------------------ 读

    pub fn list_notes(&self) -> Result<Vec<NoteSummary>, VaultError> {
        let index = self.load_index()?;
        let mut out: Vec<NoteSummary> = index
            .values()
            .map(|entry| NoteSummary {
                key: format!("{}:{}", entry.ns, entry.title),
                title: ParsedTitle {
                    ns: entry.ns,
                    title: entry.title.clone(),
                }
                .display(&self.table),
                rev: entry.rev,
                modified: entry.at.clone(),
            })
            .collect();
        out.sort_by(|a, b| a.title.cmp(&b.title));
        Ok(out)
    }

    /// 读一篇笔记。**目标不存在不算错误**，而是返回一个交给界面处理的结果
    /// （前端据此显示「还没有这篇笔记」与创建按钮）。
    pub fn load_outcome(&self, title: &str) -> Result<LoadOutcome, VaultError> {
        let parsed = self.table.parse(title, self.config.capital_links)?;
        let display = parsed.display(&self.table);

        let index = self.load_index()?;
        let Some(entry) = index.get(&parsed.key()).cloned() else {
            return Ok(LoadOutcome {
                note: None,
                title: display,
                deleted: false,
            });
        };

        let events = self.read_events(&entry.id)?;
        let state = fold(&events);
        if state.deleted {
            return Ok(LoadOutcome {
                note: None,
                title: display,
                deleted: true,
            });
        }

        let markdown_text = match &state.blob {
            Some(blob) => String::from_utf8(self.blobs.get(blob)?)
                .map_err(|_| VaultError::NotText(display.clone()))?,
            None => String::new(),
        };

        let resolver = self.resolver(&index, Some(parsed.clone()));
        let html = markdown::render_with(&markdown_text, Some(&resolver));

        Ok(LoadOutcome {
            note: Some(Note {
                key: parsed.key(),
                title: state.display(&self.table),
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
        let (_, entry) = self.find(title)?;
        let events = self.read_events(&entry.id)?;

        let mut out = Vec::new();
        let mut previous_bytes = 0u64;

        for event in &events {
            let (rev, kind, at, bytes, supersedes, summary) = match event {
                Event::Meta { at, .. } => (0, "create", at.clone(), 0u64, Vec::new(), None),
                Event::Rev {
                    at,
                    rev,
                    bytes,
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
                ),
                Event::Auto {
                    at, rev, bytes, on, ..
                } => (
                    *rev,
                    "draft",
                    at.clone(),
                    *bytes,
                    Vec::new(),
                    Some(format!("自动保存（基于版本 {on}）")),
                ),
                Event::Del { at, rev, summary } => (
                    *rev,
                    "delete",
                    at.clone(),
                    0u64,
                    Vec::new(),
                    summary.clone(),
                ),
            };

            // 只有带正文的记录才谈得上「比上一条多了多少字节」
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
        let (_, entry) = self.find(title)?;
        let events = self.read_events(&entry.id)?;

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
                title: entry.title.clone(),
                rev,
            });
        };
        if blob.is_empty() {
            return Err(VaultError::RevisionNotFound {
                title: entry.title.clone(),
                rev,
            });
        }

        let markdown_text = String::from_utf8(self.blobs.get(&blob)?)
            .map_err(|_| VaultError::NotText(entry.title.clone()))?;
        let parsed = ParsedTitle { ns, title: page };
        let index = self.load_index()?;
        let resolver = self.resolver(&index, Some(parsed.clone()));
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
        let parsed = self.table.parse(title, self.config.capital_links)?;
        let mut index = self.load_index()?;
        if index.contains_key(&parsed.key()) {
            return self.load(title);
        }

        let id = self.new_id()?;
        let at = now_iso();
        self.append(
            &id,
            &Event::Meta {
                at: at.clone(),
                ns: parsed.ns,
                title: parsed.title.clone(),
            },
        )?;

        index.insert(
            parsed.key(),
            IndexEntry {
                id,
                ns: parsed.ns,
                title: parsed.title.clone(),
                rev: 0,
                at,
            },
        );
        self.save_index(&index)?;
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
        let parsed = self.table.parse(title, self.config.capital_links)?;
        let key = parsed.key();
        let mut index = self.load_index()?;

        let (id, events) = match index.get(&key) {
            Some(entry) => (entry.id.clone(), self.read_events(&entry.id)?),
            None => {
                let id = self.new_id()?;
                let at = now_iso();
                self.append(
                    &id,
                    &Event::Meta {
                        at,
                        ns: parsed.ns,
                        title: parsed.title.clone(),
                    },
                )?;
                (id, Vec::new())
            }
        };

        let state = fold(&events);
        if state.rev != base_rev {
            return Err(VaultError::Conflict {
                expected: base_rev,
                found: state.rev,
            });
        }

        let blob = self.blobs.put(markdown_text.as_bytes())?;
        // 本次提交取代了挂在当前版本上的所有草稿
        let supersedes: Vec<u64> = events
            .iter()
            .filter_map(|event| match event {
                Event::Auto { rev, on, .. } if *on == state.rev => Some(*rev),
                _ => None,
            })
            .collect();

        let rev = next_rev(&events);
        let at = now_iso();
        self.append(
            &id,
            &Event::Rev {
                at: at.clone(),
                rev,
                blob,
                bytes: markdown_text.len() as u64,
                mime: DEFAULT_MIME.to_string(),
                parent: (state.rev > 0).then_some(state.rev),
                supersedes,
                ns: None,
                title: None,
                summary: summary.map(str::to_string),
            },
        )?;

        index.insert(
            key,
            IndexEntry {
                id,
                ns: parsed.ns,
                title: parsed.title.clone(),
                rev,
                at,
            },
        );
        self.save_index(&index)?;
        self.load(title)
    }

    /// 改名 / 迁移命名空间：追加一条提交，内容不变，历史不断
    pub fn rename(&self, from: &str, to: &str) -> Result<Note, VaultError> {
        let (_, entry) = self.find(from)?;
        let target = self.table.parse(to, self.config.capital_links)?;
        let mut index = self.load_index()?;

        if let Some(existing) = index.get(&target.key()) {
            if existing.id != entry.id {
                return Err(VaultError::Conflict {
                    expected: 0,
                    found: existing.rev,
                });
            }
        }

        let events = self.read_events(&entry.id)?;
        let state = fold(&events);
        if state.deleted {
            return Err(VaultError::Deleted(state.display(&self.table)));
        }
        let Some(blob) = state.blob.clone() else {
            return Err(VaultError::NotFound(state.display(&self.table)));
        };

        let rev = next_rev(&events);
        let at = now_iso();
        self.append(
            &entry.id,
            &Event::Rev {
                at: at.clone(),
                rev,
                blob,
                bytes: 0,
                mime: state.mime.clone(),
                parent: (state.rev > 0).then_some(state.rev),
                supersedes: Vec::new(),
                ns: Some(target.ns),
                title: Some(target.title.clone()),
                summary: Some(format!("改名：{}", target.display(&self.table))),
            },
        )?;

        index.remove(&state.key());
        index.insert(
            target.key(),
            IndexEntry {
                id: entry.id,
                ns: target.ns,
                title: target.title.clone(),
                rev,
                at,
            },
        );
        self.save_index(&index)?;
        self.load(to)
    }

    /// 删除：写一条删除标记，不抹除历史
    pub fn delete(&self, title: &str) -> Result<(), VaultError> {
        let (parsed, entry) = self.find(title)?;
        let events = self.read_events(&entry.id)?;
        let rev = next_rev(&events);
        self.append(
            &entry.id,
            &Event::Del {
                at: now_iso(),
                rev,
                summary: None,
            },
        )?;

        let mut index = self.load_index()?;
        index.remove(&parsed.key());
        self.save_index(&index)?;
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
        let (_, entry) = self.find(title)?;
        let events = self.read_events(&entry.id)?;
        let state = fold(&events);
        if state.rev != base_rev {
            return Err(VaultError::Conflict {
                expected: base_rev,
                found: state.rev,
            });
        }

        let blob = hash_bytes(markdown_text.as_bytes());
        // 内容没变就不追加，免得自动保存把日志灌满
        if let Some(draft) = &state.draft {
            if draft.blob == blob {
                return Ok(());
            }
        }

        let rev = next_rev(&events);
        self.blobs.put(markdown_text.as_bytes())?;
        self.append(
            &entry.id,
            &Event::Auto {
                at: now_iso(),
                rev,
                blob,
                bytes: markdown_text.len() as u64,
                mime: DEFAULT_MIME.to_string(),
                on: base_rev,
            },
        )
    }

    pub fn load_draft(&self, title: &str) -> Result<Option<Draft>, VaultError> {
        let (_, entry) = self.find(title)?;
        let events = self.read_events(&entry.id)?;
        let state = fold(&events);
        let Some(draft) = state.draft else {
            return Ok(None);
        };
        let text = String::from_utf8(self.blobs.get(&draft.blob)?)
            .map_err(|_| VaultError::NotText(entry.title.clone()))?;
        Ok(Some(Draft {
            markdown: text,
            base_rev: draft.on,
            at: draft.at,
        }))
    }

    /// 丢弃当前草稿：把挂在当前版本上的草稿节点从日志里删掉
    pub fn discard_draft(&self, title: &str) -> Result<usize, VaultError> {
        let (_, entry) = self.find(title)?;
        let events = self.read_events(&entry.id)?;
        let state = fold(&events);
        let current = state.rev;
        self.rewrite_log(&entry.id, |event| {
            !matches!(event, Event::Auto { on, .. } if *on == current)
        })
    }

    /// 清理已被提交取代的草稿节点。随时可做；不做也不影响正确性。
    pub fn prune(&self, title: &str) -> Result<usize, VaultError> {
        let (_, entry) = self.find(title)?;
        let events = self.read_events(&entry.id)?;
        let state = fold(&events);
        let superseded = state.superseded.clone();
        self.rewrite_log(&entry.id, |event| match event {
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

    // ------------------------------------------------------------ 首次运行

    /// 仓库里一篇笔记都没有时，用给定内容建一篇（保证首次启动有东西可看）
    pub fn seed_if_empty(&self, title: &str, markdown_text: &str) -> Result<bool, VaultError> {
        if !self.load_index()?.is_empty() {
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

fn now_iso() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

fn to_base36(mut value: u64) -> String {
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if value == 0 {
        return "0".to_string();
    }
    let mut buf = Vec::new();
    while value > 0 {
        buf.push(DIGITS[(value % 36) as usize]);
        value /= 36;
    }
    buf.reverse();
    String::from_utf8(buf).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    /// 测试用的临时仓库（放在临时目录，跑完删掉）
    struct TempVault {
        vault: Vault,
        root: PathBuf,
    }

    impl TempVault {
        fn new() -> Self {
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "refind-vault-{}-{n}",
                std::process::id()
            ));
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
        assert!(temp.root.join("vault.json").is_file());
        assert!(temp.root.join("namespaces.json").is_file());
        assert!(temp.root.join("notes").is_dir());
        assert!(temp.root.join("blobs").is_dir());
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
        assert_eq!(note.markdown, "# 标题\n\n正文");
        assert!(note.html.contains("<h1"), "{}", note.html);

        // 重新加载（走索引缓存）
        let again = temp.vault.load("测试条目").unwrap();
        assert_eq!(again.rev, 1);
        assert_eq!(again.markdown, "# 标题\n\n正文");
    }

    #[test]
    fn version_chain_keeps_parents_on_commits() {
        let temp = TempVault::new();
        temp.vault.create("链条").unwrap();
        temp.vault.commit("链条", "v1", None, 0).unwrap();
        temp.vault.commit("链条", "v2", None, 1).unwrap();
        temp.vault.commit("链条", "v3", None, 2).unwrap();

        let note = temp.vault.load("链条").unwrap();
        assert_eq!(note.rev, 3);
        assert_eq!(note.markdown, "v3");
    }

    #[test]
    fn draft_is_chained_but_superseded_by_commit() {
        let temp = TempVault::new();
        temp.vault.create("草稿").unwrap();
        temp.vault.commit("草稿", "v1", None, 0).unwrap();

        // 两次自动保存 → 链上出现两个草稿节点（版本 2、3）
        temp.vault.save_draft("草稿", "v1-草稿a", 1).unwrap();
        temp.vault.save_draft("草稿", "v1-草稿b", 1).unwrap();

        let draft = temp.vault.load_draft("草稿").unwrap().unwrap();
        assert_eq!(draft.markdown, "v1-草稿b");
        assert_eq!(draft.base_rev, 1);

        // 提交：版本号接在草稿之后（4），但 parent 仍然指向上一个提交（1）
        let note = temp.vault.commit("草稿", "v1-草稿b", Some("提交"), 1).unwrap();
        assert_eq!(note.rev, 4);
        assert!(temp.vault.load_draft("草稿").unwrap().is_none());

        // 日志里记录了本次提交取代了 2、3 两个草稿节点
        let events = temp.vault.read_events(&temp.vault.find("草稿").unwrap().1.id).unwrap();
        let supersedes = events
            .iter()
            .find_map(|event| match event {
                Event::Rev { rev: 4, supersedes, .. } => Some(supersedes.clone()),
                _ => None,
            })
            .expect("找到提交 4");
        assert_eq!(supersedes, vec![2, 3]);

        let parent = events
            .iter()
            .find_map(|event| match event {
                Event::Rev { rev: 4, parent, .. } => Some(*parent),
                _ => None,
            })
            .unwrap();
        assert_eq!(parent, Some(1), "parent 必须指向上一个提交，而不是草稿");
    }

    #[test]
    fn prune_removes_superseded_drafts_without_breaking_the_chain() {
        let temp = TempVault::new();
        temp.vault.create("清理").unwrap();
        temp.vault.commit("清理", "v1", None, 0).unwrap();
        temp.vault.save_draft("清理", "draft-a", 1).unwrap();
        temp.vault.save_draft("清理", "draft-b", 1).unwrap();
        temp.vault.commit("清理", "v2", None, 1).unwrap();

        let removed = temp.vault.prune("清理").unwrap();
        assert_eq!(removed, 2, "两个草稿节点应当被清掉");

        // 提交链完好。注意版本号是一条序列，草稿也占号，所以这次提交是 4 而不是 2。
        let note = temp.vault.load("清理").unwrap();
        assert_eq!(note.rev, 4);
        assert_eq!(note.markdown, "v2");

        let events = temp.vault.read_events(&temp.vault.find("清理").unwrap().1.id).unwrap();
        assert!(!events.iter().any(|event| matches!(event, Event::Auto { .. })));
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
        let events = temp.vault.read_events(&temp.vault.find("幂等").unwrap().1.id).unwrap();
        let autos = events
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

        let error = temp.vault.commit("冲突", "v2", None, 0).unwrap_err();
        assert!(matches!(
            error,
            VaultError::Conflict {
                expected: 0,
                found: 1
            }
        ));
    }

    #[test]
    fn index_is_only_a_cache_and_can_be_rebuilt() {
        let temp = TempVault::new();
        temp.vault.create("甲").unwrap();
        temp.vault.commit("甲", "内容甲", None, 0).unwrap();
        temp.vault.create("乙").unwrap();
        temp.vault.commit("乙", "内容乙", None, 0).unwrap();

        // 删掉缓存，直接从日志重放
        fs::remove_file(temp.root.join("index.json")).unwrap();
        let notes = temp.vault.list_notes().unwrap();
        assert_eq!(notes.len(), 2);
        // 排序按 Unicode 码点，不是拼音：乙(U+4E59) 排在 甲(U+7532) 前面
        assert_eq!(notes[0].title, "乙");
        assert_eq!(notes[1].title, "甲");
        assert!(temp.root.join("index.json").is_file(), "应当重建缓存");

        // 缓存写坏也一样
        fs::write(temp.root.join("index.json"), b"{ not json").unwrap();
        assert_eq!(temp.vault.list_notes().unwrap().len(), 2);
    }

    #[test]
    fn rename_keeps_the_history() {
        let temp = TempVault::new();
        temp.vault.create("旧名").unwrap();
        temp.vault.commit("旧名", "正文", None, 0).unwrap();

        let note = temp.vault.rename("旧名", "新名").unwrap();
        assert_eq!(note.key, "0:新名");
        assert_eq!(note.title, "新名");
        assert_eq!(note.rev, 2, "改名也是一次提交");
        assert_eq!(note.markdown, "正文", "内容不变");

        assert!(matches!(
            temp.vault.load("旧名"),
            Err(VaultError::NotFound(_))
        ));
        // 日志文件没有搬家，历史还在
        let ids = temp.vault.note_ids().unwrap();
        assert_eq!(ids.len(), 1);
    }

    #[test]
    fn delete_writes_a_marker_not_erases_history() {
        let temp = TempVault::new();
        temp.vault.create("待删").unwrap();
        temp.vault.commit("待删", "正文", None, 0).unwrap();
        temp.vault.delete("待删").unwrap();

        assert!(matches!(temp.vault.load("待删"), Err(VaultError::NotFound(_))));
        // 日志仍在，且折叠出的是「已删除」
        let id = temp.vault.note_ids().unwrap().remove(0);
        let state = fold(&temp.vault.read_events(&id).unwrap());
        assert!(state.deleted);
        assert_eq!(state.blob.as_deref().map(|b| !b.is_empty()), Some(true));
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

    /// 钉住磁盘上的格式：事件流是 JSONL、一行一个动作，索引只是缓存。
    /// 这条是「以后能完全重放」的前提，格式一旦变了就得显式改这里。
    #[test]
    fn log_format_is_stable() {
        let temp = TempVault::new();
        temp.vault.seed_if_empty("格式", "内容").unwrap();
        temp.vault.save_draft("格式", "草稿", 1).unwrap();
        temp.vault
            .commit("格式", "定稿", Some("提交"), 1)
            .unwrap();

        let id = temp.vault.note_ids().unwrap().remove(0);
        let log = fs::read_to_string(temp.root.join("notes").join(format!("{id}.log"))).unwrap();
        let index = fs::read_to_string(temp.root.join("index.json")).unwrap();

        println!("---- notes/{id}.log ----\n{log}---- index.json ----\n{index}");

        let lines: Vec<&str> = log.lines().collect();
        assert_eq!(lines.len(), 4, "meta + rev1 + auto + rev2");
        assert!(lines[0].contains(r#""t":"meta""#), "{}", lines[0]);
        assert!(lines[1].contains(r#""t":"rev""#), "{}", lines[1]);
        assert!(lines[2].contains(r#""t":"auto""#), "{}", lines[2]);
        assert!(lines[2].contains(r#""on":1"#), "{}", lines[2]);

        // 第二次提交取代了挂在版本 1 上的草稿（版本 2）
        assert!(lines[3].contains(r#""t":"rev""#), "{}", lines[3]);
        assert!(lines[3].contains(r#""rev":3"#), "{}", lines[3]);
        assert!(lines[3].contains(r#""supersedes":[2]"#), "{}", lines[3]);
        assert!(lines[3].contains(r#""parent":1"#), "{}", lines[3]);
    }

    /// 仓库目录不存在——连父目录都不存在——时也要能自动建起来。
    ///
    /// 应用是凭空在用户主目录下造出 `~/.refind-note` 的，这条不能指望事先准备目录。
    #[test]
    fn opens_and_creates_a_missing_vault() {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let base = std::env::temp_dir().join(format!("refind-make-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let root = base.join("nested").join("deeper");

        assert!(!root.exists(), "前提：这个目录一开始不存在");
        let vault = Vault::open(&root).expect("应当自动建库");

        assert!(root.join("notes").is_dir(), "notes/ 应当被建出来");
        assert!(root.join("blobs").is_dir(), "blobs/ 应当被建出来");
        assert!(root.join("vault.json").is_file(), "应当写入默认设置");
        assert!(root.join("namespaces.json").is_file(), "应当写入内建命名空间");
        // 建完就能直接用
        assert!(vault.list_notes().unwrap().is_empty());

        // 顺手打印出来，方便人工核对（`cargo test -- --nocapture` 时可见）
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

    /// 默认仓库必须落在用户主目录下的 `.refind-note`
    #[test]
    fn default_root_is_under_home() {
        let root = default_root().expect("应当能定位到主目录");
        assert_eq!(
            root.file_name().and_then(|name| name.to_str()),
            Some(DEFAULT_DIR_NAME)
        );

        if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
            assert!(
                root.starts_with(&home),
                "{} 应当在家目录 {:?} 之下",
                root.display(),
                home
            );
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
        assert_eq!(history[3].supersedes, vec![2], "第二次提交取代了那条草稿");

        // delta 是相对上一条记录的增减：「第一版」是 3 个 CJK 字 = 9 字节
        assert_eq!(history[1].bytes, 9);
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

        // 草稿也在同一条版本序列里，所以同样取得到
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

    #[test]
    fn a_renamed_version_keeps_the_title_of_its_time() {
        let temp = TempVault::new();
        temp.vault.create("旧标题").unwrap();
        temp.vault.commit("旧标题", "正文", None, 0).unwrap();
        temp.vault.rename("旧标题", "新标题").unwrap();

        // 改名本身就是一次提交（同一个 blob，标题变了）
        let old = temp.vault.revision("新标题", 1).unwrap();
        assert_eq!(old.title, "旧标题", "版本 1 当时的标题还是旧的");
        let new = temp.vault.revision("新标题", 2).unwrap();
        assert_eq!(new.title, "新标题");
        assert_eq!(new.markdown, "正文");
    }
}
