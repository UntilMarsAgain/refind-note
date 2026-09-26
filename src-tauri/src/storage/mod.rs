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
pub use crate::title::Namespace;
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
    Address, CommandInfo, DiffResult, Draft, GcReport, LoadOutcome, MaintenanceReport, Note,
    NoteSummary, PurgeReport, RevisionContent, RevisionSummary, TrashEntry, VaultSettings, Via,
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
    /// 仓库级设置（`vault.json`）：影响数据语义
    config: VaultConfig,
    /// 界面偏好（`preferences.json`）：不影响数据语义，同步时整体排除
    preferences: crate::storage::config::Appearance,
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

        // 界面偏好单独一个文件；不存在就写一份默认的
        let preferences_path = root.join("preferences.json");
        let preferences: crate::storage::config::Appearance =
            match fs::read_to_string(&preferences_path) {
                Ok(text) => serde_json::from_str(&text)?,
                Err(_) => {
                    let preferences = crate::storage::config::Appearance::default();
                    write_atomic(&preferences_path, &serde_json::to_vec_pretty(&preferences)?)?;
                    preferences
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
                    preferences,
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

    /// 名字表里存的是**完整显示标题**（`help:甲`），而 `ParsedTitle.title` 是命名空间内的
    /// **裸标题**（`甲`）。两种表示混用会出大问题：`display()` 会再拼一次前缀，变成
    /// `help:help:甲`，下一次解析就撞上"标题里有冒号"。这里统一剥掉前缀，只此一处。
    fn bare_title(display: &str, table: &NamespaceTable, ns: &str) -> String {
        let Some((prefix, rest)) = display.split_once(':') else {
            return display.to_string();
        };
        match table.lookup(prefix) {
            // 前缀确实指向这个命名空间（含别名、大小写）→ 剥掉它
            Some(item) if item.id == ns => rest.to_string(),
            _ => display.to_string(),
        }
    }

    /// 一篇笔记**将要**落在哪：新建时用（文件还不存在，只能按命名空间拼）。
    ///
    /// 命名空间在这里真正进入路径。以前 `log_path` 硬编码主命名空间，于是所有新笔记都落进
    /// `notes/0/` —— 两跳寻址在"落到磁盘"这一跳就断了（`walk` 从目录名反推命名空间，
    /// 看到的一律是 `0`，于是清空/删除命名空间永远找不到东西）。
    fn note_path(&self, ns: &str, id: &str) -> PathBuf {
        self.notes_dir()
            .join(ns)
            .join(format!("{id}.{LOG_EXT}"))
    }

    /// 在某个目录下按 id 找现成的日志。
    ///
    /// 命名空间被删掉之后，标题前缀就解析不出标识了 —— 那时只有这个办法找得回它
    /// （回收站里的记录还得能被清除，否则永久卡住）。
    fn find_log(base: &Path, id: &str) -> Option<PathBuf> {
        let file = format!("{id}.{LOG_EXT}");
        for entry in fs::read_dir(base).ok()?.flatten() {
            let candidate = entry.path().join(&file);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        None
    }

    /// 某个 id 属于哪个命名空间：走**名字表**（id → 显示标题 → 前缀 → 表里的标识）。
    ///
    /// 新建的笔记在落盘前就登记了名字，所以这条路对 `append` / `read_events` 那些
    /// 只拿得到 id 的地方都成立，不必给它们改签名。
    fn ns_of_id(&self, id: &str) -> String {
        let Ok(titles) = self.titles() else {
            return MAIN_NAMESPACE_DIR.to_string();
        };
        let Some(display) = titles.notes.get(id).or_else(|| titles.trashed.get(id)) else {
            return MAIN_NAMESPACE_DIR.to_string();
        };
        match display.split_once(':') {
            Some((prefix, _)) => self
                .table
                .lookup(prefix)
                .map(|item| item.id.clone())
                .unwrap_or_else(|| MAIN_NAMESPACE_DIR.to_string()),
            None => MAIN_NAMESPACE_DIR.to_string(),
        }
    }

    /// 笔记文件路径。`id` 是生成出来的十六进制串，**推不出命名空间**，所以由名字表给。
    fn log_path(&self, id: &str) -> PathBuf {
        let path = self.note_path(&self.ns_of_id(id), id);
        if path.is_file() {
            return path;
        }
        Self::find_log(&self.notes_dir(), id).unwrap_or(path)
    }

    /// 已删除的笔记挪到这里：历史保留，将来可以接恢复
    fn trashed_path(&self, id: &str) -> PathBuf {
        let path = self
            .trash_dir()
            .join(self.ns_of_id(id))
            .join(format!("{id}.{LOG_EXT}"));
        if path.is_file() {
            return path;
        }
        Self::find_log(&self.trash_dir(), id).unwrap_or(path)
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
            // 名字表里没有、两个目录里也翻不到，才算没人用过这个 id
            if !titles.notes.contains_key(&id)
                && !titles.trashed.contains_key(&id)
                && Self::find_log(&self.notes_dir(), &id).is_none()
                && Self::find_log(&self.trash_dir(), &id).is_none()
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
        // 新建的笔记还没落盘，路径只能按解析出的命名空间拼
        Ok((
            parsed.clone(),
            self.note_path(&parsed.ns, &id),
        ))
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

            // 目录名就是命名空间标识（**字符串**）。主命名空间是 "0"（没有前缀时占位）
            let Some(ns) = namespace_path
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_string)
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
                out.push(ParsedTitle {
                    ns: ns.clone(),
                    title: Self::bare_title(&title, &self.table, &ns),
                });
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
        let appearance = self.preferences();
        VaultSettings {
            root: self.root.display().to_string(),
            format: self.config.format,
            capital_links: self.config.capital_links,
            max_title_bytes: self.config.max_title_bytes,
            delta_chain_limit: self.config.delta_chain_limit,
            trash_keep_days: self.config.trash_keep_days,
            gc_interval_days: self.config.gc_interval_days,
            last_trash_purge: self.config.last_trash_purge.clone(),
            last_gc: self.config.last_gc.clone(),
            theme: appearance.theme.clone(),
            accent: appearance.accent.clone(),
            reading_width: appearance.reading_width,
            zoom: appearance.zoom,
        }
    }

    /// 改仓库级设置。只接受明确给出的字段，其余保持不变。
    pub fn update_settings(
        &mut self,
        capital_links: Option<bool>,
        max_title_bytes: Option<usize>,
        delta_chain_limit: Option<usize>,
        trash_keep_days: Option<u64>,
        gc_interval_days: Option<u64>,
        theme: Option<String>,
        accent: Option<String>,
        reading_width: Option<u32>,
        zoom: Option<f64>,
    ) -> Result<(), VaultError> {
        if let Some(value) = capital_links {
            self.config.capital_links = value;
        }
        if let Some(value) = max_title_bytes {
            self.config.max_title_bytes = value;
        }
        if let Some(value) = delta_chain_limit {
            // 下限留 1：0 会让每一版都退回整份快照（合法但没意义）
            self.config.delta_chain_limit = value.max(1);
        }
        if let Some(value) = trash_keep_days {
            // 下限 1 天：填 0 会变成"每次启动都清空回收站"，那不是配置项该有的后果
            self.config.trash_keep_days = value.max(1);
        }
        if let Some(value) = gc_interval_days {
            self.config.gc_interval_days = value.max(1);
        }
        if let Some(value) = theme {
            self.preferences.theme = value;
        }
        if let Some(value) = accent {
            self.preferences.accent = value;
        }
        if let Some(value) = reading_width {
            self.preferences.reading_width = value;
        }
        if let Some(value) = zoom {
            // 夹在合理区间：太小读不了，太大等于把界面推出屏幕
            self.preferences.zoom = value.clamp(0.5, 3.0);
        }

        // 两条落盘路径，各写各的文件：数据语义进 vault.json、界面偏好进 preferences.json
        self.save_config()?;
        self.save_preferences()
    }

    /// 渲染任意 markdown：编辑器右侧的预览用它。
    ///
    /// 刻意复用**阅读视图同一个渲染器**（`markdown::render_with` + 当前仓库的链接解析）：
    /// 预览自己再实现一套渲染，就会和正文各说各话 —— 那正是本项目反复吃亏的"第二个真相"。
    pub fn render(&self, markdown: &str) -> String {
        let resolver = self.resolver(None);
        crate::markdown::render_with(markdown, Some(&resolver))
    }

    /// 界面偏好（主题、主题色、限宽）
    pub fn preferences(&self) -> &crate::storage::config::Appearance {
        &self.preferences
    }

    /// 数据语义设置：`~/.refind-note/vault.json`
    fn save_config(&self) -> Result<(), VaultError> {
        let path = self.root.join("vault.json");
        write_atomic(&path, &serde_json::to_vec_pretty(&self.config)?)?;
        Ok(())
    }

    /// 界面偏好：`~/.refind-note/preferences.json`（同步/备份时整体排除）
    fn save_preferences(&self) -> Result<(), VaultError> {
        let path = self.root.join("preferences.json");
        write_atomic(&path, &serde_json::to_vec_pretty(&self.preferences)?)?;
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
        let mut out: Vec<NoteSummary> = Vec::new();

        for parsed in self.walk(&self.notes_dir())? {
            let display = parsed.display(&self.table);
            // 标注指令页面：要判定"第一行是不是标记"就得读一遍当前正文，
            // 这是唯一的办法（代价是列表页对每篇笔记多一次读取）。
            let command = match self.current_markdown(&display)? {
                Some(markdown) => CommandInfo::from_parsed(&crate::command::parse(&markdown)),
                None => None,
            };
            out.push(NoteSummary {
                key: parsed.key(),
                title: display,
                command,
            });
        }

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

        let mut ns = crate::title::MAIN_NS.to_string();
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
                    ns = meta_ns.clone();
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
                        ns = value.clone();
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
        self.parse_address_at(input, 0, None)
    }

    /// 真正的解析。`hops` 是已经跟过几跳重定向（对外一律从 0 起步）。
    fn parse_address_at(
        &self,
        input: &str,
        hops: usize,
        via: Option<Via>,
    ) -> Result<Address, VaultError> {
        let raw = input.trim();
        if raw.is_empty() {
            return Ok(Address::Empty);
        }

        // 虚拟命名空间（`special:` 及其别名）：不对应笔记文件，交给前端渲染特殊页面。
        // 优先于笔记形态判断；标题里不允许冒号，所以不可能有笔记叫这个名字。
        //
        // **走命名空间表，而不是写死 `"special:"`** —— 给 special 配了别名之后，别名同样
        // 要能进这一支。写死的话 `特殊:settings` 会掉进普通笔记解析，最后报「标题里有冒号」，
        // 让人以为别名没配上。
        //
        // 顺带说明为什么只认 special：跨站命名空间也是"非存储"的，但它的地址不该被当成
        // 特殊页面（那会跑到不存在的页面上）。
        let special = raw
            .split(['@', '#'])
            .next()
            .unwrap_or("")
            .split_once(':')
            .and_then(|(prefix, rest)| {
                self.table
                    .lookup(prefix)
                    .filter(|item| item.id == crate::title::SPECIAL_NS)
                    .map(|_| rest.trim().to_string())
            });

        if let Some(rest) = special {
            // 特殊页面**没有状态**：`special:newtab@edit` 这类把状态裁掉（不当错误）。
            let page = rest.trim().to_ascii_lowercase();
            let section = raw
                .split_once('#')
                .map(|(_, tail)| tail.trim())
                .filter(|value| !value.is_empty());

            if page.is_empty() {
                return Err(VaultError::BadAddress(
                    "special: 后面要写页面名，例如 special:newtab".to_string(),
                ));
            }

            // 不存在的特殊页面要**明确报不存在**，而不是当普通笔记去找
            if !SPECIAL_PAGES.contains(&page.as_str()) {
                return Err(VaultError::BadAddress(format!(
                    "没有这个特殊页面：special:{page}（现有：{}）",
                    SPECIAL_PAGES.join("、")
                )));
            }

            // `special:random` 不是"一页"，而是**一次跳转**：随机落到主命名空间的某一篇。
            // 与 RANDOM_REDIRECT 共用 `random_title`，两条路的行为不会分家。
            if page == "random" {
                self.guard_hops(&format!("special:{page}"), hops)?;
                // 没有"当前页"要排除，传空串即可（没有笔记会叫这个名字）
                let target = self.pick_random_title(None, "")?;
                return self.parse_address_at(
                    &target,
                    hops + 1,
                    Some(Via {
                        from: format!("special:{page}"),
                        random: true,
                    }),
                );
            }

            return Ok(Address::Special {
                address: compose_address(&format!("special:{page}"), None, section),
                page,
                via,
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

        // `no-command` 自己带一个连字符，所以必须在拆分 `head-reference` **之前**识别。
        if state.eq_ignore_ascii_case("no-command") {
            let is_command = self
                .current_markdown(&display)?
                .map(|markdown| {
                    !matches!(
                        crate::command::parse(&markdown),
                        crate::command::Parsed::None
                    )
                })
                .unwrap_or(false);

            // 一般页面：这个状态没有意义 —— 回显时裁掉，当作没写
            if !is_command {
                return Ok(Address::Note {
                    address: compose_address(&display, None, section.as_deref()),
                    title: display,
                    via: None,
                    code_block: false,
                });
            }

            // 指令页面：不执行指令，原样显示它自己的内容（前端按 code_block 包成代码块）
            return Ok(Address::Note {
                address: compose_address(&display, Some("no-command"), section.as_deref()),
                title: display,
                via: None,
                code_block: true,
            });
        }

        let (head, reference) = match state.split_once('-') {
            Some((head, reference)) => (head.to_ascii_lowercase(), Some(reference.trim())),
            None => (state.to_ascii_lowercase(), None),
        };

        match (head.as_str(), reference) {
            // 默认：看最新提交
            ("", _) | ("view", None) => {
                // 指令页面：**只有这一路跟重定向**。@edit / @history / @delete 操作的是
                // 这一页本身，跟着跳走会让人删错页面。跳数上限在 command_target 里把关。
                if let Some(chase) = self.command_target(&display, hops)? {
                    // 记下"从哪儿来"：落到目标页后，标题下方要提示（随机跳转不写名字）
                    return self.parse_address_at(
                        &chase.target,
                        hops + 1,
                        Some(Via {
                            from: display.clone(),
                            random: chase.random,
                        }),
                    );
                }

                Ok(Address::Note {
                    address: compose_address(&display, None, section.as_deref()),
                    title: display,
                    via,
                    code_block: false,
                })
            }
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
                    // 看最新提交等价于"直接看这篇"，所以同样要跟重定向
                    if let Some(chase) = self.command_target(&display, hops)? {
                        return self.parse_address_at(
                            &chase.target,
                            hops + 1,
                            Some(Via {
                                from: display.clone(),
                                random: chase.random,
                            }),
                        );
                    }
                    return Ok(Address::Note {
                        address: compose_address(&display, None, section.as_deref()),
                        title: display,
                        via,
                        code_block: false,
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
                "不认识「@{state}」；状态只有 edit / history / delete / view-版本 / rollback-版本 / no-command"
            ))),
        }
    }

    /// 命名空间表（设置页要看）
    pub fn namespaces(&self) -> Vec<Namespace> {
        self.table.items.clone()
    }

    /// 把命名空间表写回去。表现在可以被用户改，所以必须落盘。
    fn save_namespaces(&self) -> Result<(), VaultError> {
        write_atomic(
            &self.root.join("namespaces.json"),
            &serde_json::to_vec_pretty(&*self.table)?,
        )?;
        Ok(())
    }

    /// 新增一个命名空间。标识就是名字本身。
    pub fn add_namespace(
        &mut self,
        name: &str,
        aliases: Vec<String>,
        site: Option<String>,
    ) -> Result<(), VaultError> {
        let mut table = (*self.table).clone();
        table
            .add(name, aliases, site)
            .map_err(VaultError::BadAddress)?;
        self.table = Arc::new(table);
        self.save_namespaces()
    }

    /// 把"名字或别名"解析成**标识**。
    ///
    /// 两跳寻址的关键一步：外面（界面、命令、地址）用名字，里面（目录、键）用标识。
    fn namespace_id(&self, name_or_alias: &str) -> String {
        let needle = name_or_alias.trim();
        // 空与 "0" 都是主命名空间
        if needle.is_empty() {
            return crate::title::MAIN_NS.to_string();
        }
        self.table
            .lookup(needle)
            .map(|item| item.id.clone())
            .unwrap_or_else(|| needle.to_string())
    }

    /// 清空一个命名空间：里面的页面全部进回收站（可还原），命名空间本身留着。
    pub fn empty_namespace(&self, key: &str) -> Result<usize, VaultError> {
        // 传进来的可能是名字（界面按名字称呼它），这里换成标识再比 —— 目录用的是标识
        let id = self.namespace_id(key);
        let titles: Vec<String> = self
            .walk(&self.notes_dir())?
            .iter()
            .filter(|parsed| parsed.ns == id)
            .map(|parsed| parsed.display(&self.table))
            .collect();

        let mut count = 0;
        for title in titles {
            // 走既有的删除路径：写删除标记、文件搬进回收站 —— 一件事只实现一次
            self.delete(&title)?;
            count += 1;
        }
        Ok(count)
    }

    /// 改一个命名空间的别名（增、删、一次给多个都用它）。
    ///
    /// 名称与标识都不动 —— 别名只是"另外几个也能写的前缀"。保留的两个（主命名空间与
    /// `special`）**也可以有别名**：给主命名空间配 `主`，`[[主:某页]]` 就落在主命名空间。
    pub fn update_namespace_aliases(
        &mut self,
        key: &str,
        aliases: Vec<String>,
    ) -> Result<(), VaultError> {
        let id = self.namespace_id(key);
        let mut table = (*self.table).clone();
        let Some(item) = table.items.iter_mut().find(|item| item.id == id) else {
            return Err(VaultError::BadAddress(format!("命名空间「{key}」不存在")));
        };
        item.aliases = aliases;
        // 先校验、通过了才替换：重名与非法字符都在这里拦下
        table.validate().map_err(VaultError::BadAddress)?;
        self.table = Arc::new(table);
        self.save_namespaces()
    }

    /// 给命名空间改名。
    ///
    /// 两跳寻址下这**只是改表里一行**：磁盘目录与标题键用的都是标识，所以
    /// **一个文件都不用动**（这也正是当初把标识与名字分开的理由）。
    /// 需要跟着走的只有名字表里的**完整显示标题**前缀。
    pub fn rename_namespace(&mut self, key: &str, new_name: &str) -> Result<(), VaultError> {
        let id = self.namespace_id(key);
        if NamespaceTable::is_reserved(&id) {
            return Err(VaultError::BadAddress(format!(
                "「{key}」是保留的命名空间，不能改名"
            )));
        }
        let old_name = self
            .table
            .get(&id)
            .map(|item| item.name.clone())
            .ok_or_else(|| VaultError::BadAddress(format!("命名空间「{key}」不存在")))?;

        let new_name = new_name.trim().to_string();
        let mut table = (*self.table).clone();
        let Some(item) = table.items.iter_mut().find(|item| item.id == id) else {
            return Err(VaultError::BadAddress(format!("命名空间「{key}」不存在")));
        };
        item.name = new_name.clone();
        // 先校验、通过了才替换：失败不留半张表
        table.validate().map_err(VaultError::BadAddress)?;
        self.table = Arc::new(table);

        if !old_name.is_empty() && old_name != new_name {
            let mut titles = self.titles()?;
            let old_prefix = format!("{old_name}:");
            let new_prefix = format!("{new_name}:");
            for map in [&mut titles.notes, &mut titles.trashed] {
                for value in map.values_mut() {
                    if let Some(rest) = value.strip_prefix(&old_prefix) {
                        *value = format!("{new_prefix}{rest}");
                    }
                }
            }
            self.save_titles(&titles)?;
        }

        self.save_namespaces()
    }

    /// 删除一个命名空间：先清空（页面进回收站），再把自己从表里去掉。
    ///
    /// 键就是名字，所以**删掉再建同名 = 没删过**（你说的那条）。
    /// 主命名空间与 `special` 不可删。
    pub fn delete_namespace(&mut self, key: &str) -> Result<usize, VaultError> {
        let id = self.namespace_id(key);
        if NamespaceTable::is_reserved(&id) {
            return Err(VaultError::BadAddress(format!(
                "「{key}」是不可删除的命名空间"
            )));
        }
        let count = self.empty_namespace(&id)?;

        let mut table = (*self.table).clone();
        table.items.retain(|item| item.id != id);
        self.table = Arc::new(table);
        self.save_namespaces()?;
        Ok(count)
    }

    /// 从回收站还原一篇笔记。
    ///
    /// 做法：文件搬回 `notes/`、名字表挪回去，然后**追加一次提交**（内容不变）——
    /// `fold` 遇到 `Rev` 会把 `deleted` 清掉，于是它重新算"活着"。
    ///
    /// 用追加而不是抹掉那条删除标记：**一条历史都不丢**（"什么时候删过"这件事仍留在链上），
    /// 这也是删除确认页对用户的承诺。
    pub fn restore_note(&self, title: &str) -> Result<Note, VaultError> {
        // 命名空间可能已经被删除：那样就还原不回来了，提示要说清是哪件事
        // （标题里不会出现 `:`，所以有冒号就一定是命名空间前缀）
        if let Some((prefix, _)) = title.split_once(':') {
            if self.table.lookup(prefix).is_none() {
                return Err(VaultError::BadAddress(format!(
                    "命名空间「{}」不存在，无法还原《{title}》",
                    prefix.trim()
                )));
            }
        }

        let (parsed, path) = self.locate(title)?;
        let display = parsed.display(&self.table);
        let id = Self::id_from_path(&path);

        let trashed = self.trashed_path(&id);
        if !trashed.is_file() {
            return Err(VaultError::NotFound(display));
        }

        let events = self.read_events_at(&trashed)?;
        let state = fold(&events);
        if !state.deleted {
            // 文件在回收站里、日志却没有删除标记：状态不一致，先别动它
            return Err(VaultError::Corrupt(format!(
                "《{display}》在回收站里，但日志里没有删除标记"
            )));
        }

        // 先把文件搬回去，再追加"还原"这一版
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::rename(&trashed, &path)?;

        let mut titles = self.titles()?;
        titles.trashed.remove(&id);
        titles.notes.insert(id.clone(), display.clone());
        self.save_titles(&titles)?;

        // 与"改名"同一套做法：复用原有的 blob，只追加一条指向它的提交
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
                ns: None,
                title: None,
                summary: Some("从回收站还原".to_string()),
            },
        )?;

        self.load(&display)
    }

    /// 立即清除回收站里的**一条**（不等保留期）。
    ///
    /// 只删日志与名字表项，**不顺手回收内容块**：那件事交给「数据库回收」那一页，
    /// 由用户决定何时回收 —— 回收站页面正好链过去。
    pub fn purge_trash_entry(&self, title: &str) -> Result<(), VaultError> {
        // 按**名字表里的显示标题**找，而不是先解析标题：命名空间可能已经被删除，
        // 那时解析会失败，可这条记录还得能被清掉 —— 否则它就永久卡在回收站里了。
        let mut titles = self.titles()?;
        let Some(id) = titles
            .trashed
            .iter()
            .find(|(_, stored)| *stored == title)
            .map(|(id, _)| id.clone())
        else {
            return Err(VaultError::NotFound(title.to_string()));
        };

        let trashed = self.trashed_path(&id);
        if !trashed.is_file() {
            return Err(VaultError::NotFound(title.to_string()));
        }
        fs::remove_file(&trashed)?;
        titles.trashed.remove(&id);
        self.save_titles(&titles)?;
        Ok(())
    }

    /// 该跑了吗：用**上次执行的时间**与现在比。
    ///
    /// 从没跑过（空字符串或解析失败）一律算"该跑了"；间隔下限 1 天，
    /// 免得 0 变成"每次启动都清一次"。
    fn maintenance_due(&self, last: &str, interval_days: u64) -> bool {
        let interval = interval_days.max(1);
        match parse_iso(last) {
            None => true,
            Some(at) => {
                let elapsed = (time::OffsetDateTime::now_utc() - at).whole_days();
                elapsed >= interval as i64
            }
        }
    }

    /// 有没有到点该做的维护。
    ///
    /// 单独一个判断，是因为**建一个什么都不做的任务本身就是误导**：任务栏弹出
    /// 「数据库维护」，用户会以为它在干活，反而看不出真实状态 —— 这一次就是这么被发现的。
    pub fn maintenance_pending(&self) -> bool {
        self.maintenance_due(
            &self.config.last_trash_purge.clone(),
            self.config.trash_keep_days,
        ) || self.maintenance_due(&self.config.last_gc.clone(), self.config.gc_interval_days)
    }

    /// 自动维护：到点了就清回收站、回收内容块，并记下这次的时间。
    ///
    /// 判定只看**上次执行时间**（不是"每次启动都跑"）：间隔之内什么都不做。
    pub fn run_maintenance(&mut self) -> Result<MaintenanceReport, VaultError> {
        let mut report = MaintenanceReport::default();
        let mut touched = false;

        if self.maintenance_due(&self.config.last_trash_purge.clone(), self.config.trash_keep_days) {
            let keep = self.config.trash_keep_days as i64;
            report.purged = Some(self.purge_trash(keep)?);
            self.config.last_trash_purge = now_iso();
            touched = true;
        }

        if self.maintenance_due(&self.config.last_gc.clone(), self.config.gc_interval_days) {
            report.gc = Some(self.gc(true, false)?);
            self.config.last_gc = now_iso();
            touched = true;
        }

        if touched {
            self.save_config()?;
        }
        Ok(report)
    }

    /// 回收站清单：删过的笔记，连同"删了多久"。
    ///
    /// 清单的**存在性**来自 `trash/` 目录，名字与删除时间来自名字表和删除标记 ——
    /// 与别处一致：目录说有没有，表说叫什么。
    pub fn list_trash(&self) -> Result<Vec<TrashEntry>, VaultError> {
        let mut out = Vec::new();

        for (id, title) in self.titles()?.trashed {
            let path = self.trashed_path(&id);
            if !path.is_file() {
                // 文件没了却还留在表里：忽略它（以目录为准），不要因此让整页打不开
                continue;
            }
            let bytes = fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
            let events = self.read_events_at(&path)?;
            let at = deletion_time(&events);
            out.push(TrashEntry {
                title,
                deleted_at: at
                    .map(|value| {
                        value
                            .format(&time::format_description::well_known::Rfc3339)
                            .unwrap_or_default()
                    })
                    .unwrap_or_default(),
                bytes,
                days_old: at.map(days_since),
            });
        }

        // 新的在前：这一页的用途是"决定要不要清"，最近删的最需要看见
        out.sort_by(|a, b| b.deleted_at.cmp(&a.deleted_at));
        Ok(out)
    }

    /// 清理回收站：删掉超过 `older_than_days` 天的条目。
    ///
    /// 传 0 表示"全部清理"（任何条目都满足"已删 0 天以上"）。
    /// 清完顺手做一次内容块回收 —— 在此之前，那些块还被回收站的日志引用着，
    /// 只有日志没了它们才真正无人引用。
    pub fn purge_trash(&self, older_than_days: i64) -> Result<PurgeReport, VaultError> {
        let mut report = PurgeReport {
            removed: 0,
            freed_bytes: 0,
            blobs: GcReport::default(),
        };

        let mut titles = self.titles()?;
        let mut purged: Vec<String> = Vec::new();

        for (id, _) in &titles.trashed {
            let path = self.trashed_path(id);
            if !path.is_file() {
                continue;
            }
            let events = self.read_events_at(&path)?;
            let Some(at) = deletion_time(&events) else {
                // 读不出删除时间就不动它：**宁可不删，也不要误删**
                continue;
            };
            if days_since(at) < older_than_days {
                continue;
            }

            report.freed_bytes += fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
            fs::remove_file(&path)?;
            report.removed += 1;
            purged.push(id.clone());
        }

        for id in purged {
            titles.trashed.remove(&id);
        }
        if report.removed > 0 {
            self.save_titles(&titles)?;
            // 日志没了，它们引用的内容块这时才成为孤块
            report.blobs = self.gc(true, false)?;
        }

        Ok(report)
    }

    /// 跳数上限统一把关：会"跟下去"的指令都先过这一关，免得新增指令时漏掉。
    fn guard_hops(&self, title: &str, hops: usize) -> Result<(), VaultError> {
        if hops >= MAX_REDIRECT_HOPS {
            return Err(VaultError::BadAddress(format!(
                "重定向超过 {MAX_REDIRECT_HOPS} 跳，可能成环（停在《{title}》）"
            )));
        }
        Ok(())
    }

    /// 在某个命名空间里随机挑一篇页面的标题。
    ///
    /// `namespace` 是**命名空间标识字符串**：空或 `0` = 主命名空间；`special` 是虚拟命名空间
    /// （页面由程序提供，不落存储）；其余按标识去目录里找。
    ///
    /// 会**排除发起随机的那一页自己** —— 否则小仓库里很容易随机到自己，
    /// 然后一路跟到跳数上限，变成一次莫名其妙的失败。
    fn pick_random_title(&self, namespace: Option<&str>, from: &str) -> Result<String, VaultError> {
        let ns = match namespace.map(str::trim) {
            None | Some("") => crate::title::MAIN_NS.to_string(),
            Some(text) => text.to_string(),
        };

        let mut candidates: Vec<String> = if self.table.is_virtual(&ns) {
            // 虚拟命名空间不落存储，候选由程序给出（目前只有 special）
            if ns == crate::title::SPECIAL_NS {
                SPECIAL_PAGES
                    .iter()
                    .map(|page| format!("{}:{page}", crate::title::SPECIAL_NS))
                    .filter(|title| title != from)
                    .collect()
            } else {
                Vec::new()
            }
        } else {
            self.walk(&self.notes_dir())?
                .iter()
                .filter(|parsed| parsed.ns == ns && parsed.display(&self.table) != from)
                .map(|parsed| parsed.display(&self.table))
                .collect()
        };

        if candidates.is_empty() {
            // 报错时说显示名，别让用户对着 "0" 发愣
            let label = if ns == crate::title::MAIN_NS {
                "主命名空间".to_string()
            } else {
                format!("命名空间 {ns}")
            };
            return Err(VaultError::BadAddress(format!(
                "《{from}》随机不到页面：{label} 里没有别的页面"
            )));
        }

        // 随机源用当前时间的纳秒：这里只要"每次不一样"，不需要密码学随机，
        // 也就不为此引依赖。
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.subsec_nanos() as usize)
            .unwrap_or(0);
        Ok(candidates.swap_remove(nanos % candidates.len()))
    }

    /// 读一篇笔记的**当前正文**（不渲染）。不存在或已删除时返回 None。
    fn current_markdown(&self, title: &str) -> Result<Option<String>, VaultError> {
        let (_, path) = self.locate(title)?;
        let id = Self::id_from_path(&path);
        if !path.is_file() {
            return Ok(None);
        }

        let events = self.read_events(&id)?;
        let state = fold(&events);
        if state.deleted {
            return Ok(None);
        }
        if state.rev == 0 {
            return Ok(Some(String::new()));
        }
        Ok(Some(self.content_of(&events, state.rev, 0)?))
    }

    /// 从一篇笔记的指令里算出**下一个地址**。
    ///
    /// 返回 `Ok(None)` 表示它根本不是指令页面，照常阅读。
    /// `hops` 是已经跟过的跳数：到上限就报错 —— 成环时给一句能看懂的提示，
    /// 总好过递归到栈溢出。
    fn command_target(&self, title: &str, hops: usize) -> Result<Option<Chase>, VaultError> {
        let Some(markdown) = self.current_markdown(title)? else {
            return Ok(None);
        };

        // 指令怎么解析、跳到哪，全由 `command.rs` 那张表决定。这里只做仓库这边的两件事：
        // 跳数上限，以及把随机能力（`CommandEnv`）递给指令。
        match crate::command::parse(&markdown) {
            // 不是指令页面：照常阅读
            crate::command::Parsed::None => Ok(None),
            crate::command::Parsed::Command(command) => {
                self.guard_hops(title, hops)?;
                let random = command.spec.kind == "random-redirect";
                match command
                    .chase(self, title)
                    .map_err(|message| VaultError::BadAddress(format!("《{title}》{message}")))?
                {
                    Some(target) => Ok(Some(Chase { target, random })),
                    // 表里标明"不跳"的指令：当普通页面读
                    None => Ok(None),
                }
            }
            // 是指令页面，但指令本身有问题 —— **不能当普通页面读**：
            // 那样一条写坏的指令会静静显示成正文，谁也不知道它没生效。
            crate::command::Parsed::Empty => Err(VaultError::BadAddress(format!(
                "《{title}》是指令页面，但没写指令（第二行应写成 {}）",
                crate::command::supported()
            ))),
            crate::command::Parsed::Unrecognized(line) => Err(VaultError::BadAddress(format!(
                "《{title}》的指令认不出来：「{}」；目前支持 {}",
                line.trim(),
                crate::command::supported()
            ))),
        }
    }

    /// 这一页的指令信息（`@no-command` 顶部提示、列表标注都用它）
    pub fn command_info(&self, title: &str) -> Result<Option<CommandInfo>, VaultError> {
        let Some(markdown) = self.current_markdown(title)? else {
            return Ok(None);
        };
        Ok(CommandInfo::from_parsed(&crate::command::parse(&markdown)))
    }

    /// 按 `@no-command` 读一篇指令页面：正文**包成一个代码块**再渲染。
    ///
    /// 这样前端不必为它单开一种视图 —— 拿到的仍是一个普通 `Note`，只是 `html` 不同；
    /// `markdown` 保持原样，所以进编辑器看到的还是原始文本。
    pub fn load_code_blocked(&self, title: &str) -> Result<LoadOutcome, VaultError> {
        let mut outcome = self.load_outcome(title)?;
        if let Some(note) = outcome.note.take() {
            let resolver = self.resolver(None);
            let html =
                markdown::render_with(&markdown::fence_code(&note.markdown), Some(&resolver));
            outcome.note = Some(Note { html, ..note });
        }
        Ok(outcome)
    }

    /// 读**某一版**，并按 `@no-command` 的规则处理：
    ///
    /// 那一版的正文若是指令页面，就包成代码块再渲染 —— 看旧版等于启用 `@no-command`
    /// （不执行指令，只让你看当时的原文）。**注意不要在 `revision()` 里做这件事**：
    /// 差异视图与它共用 `revision()`，包了代码块会把 diff 弄脏。
    pub fn revision_code_blocked(
        &self,
        title: &str,
        rev: u64,
    ) -> Result<RevisionContent, VaultError> {
        let mut content = self.revision(title, rev)?;
        if !matches!(
            crate::command::parse(&content.markdown),
            crate::command::Parsed::None
        ) {
            let resolver = self.resolver(None);
            content.html = markdown::render_with(
                &markdown::fence_code(&content.markdown),
                Some(&resolver),
            );
        }
        Ok(content)
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
            return Err(VaultError::BadAddress(format!(
                "《{}》的版本引用是空的",
                parsed.title
            )));
        }

        let digits = reference.chars().all(|ch| ch.is_ascii_digit());

        // 不足 5 位：只能当版本号。缩写按约定至少 5 位，太短给明确提示。
        if reference.len() < MIN_SHORT_ID {
            if digits {
                return reference.parse::<u64>().map_err(|_| {
                    VaultError::BadAddress(format!(
                        "《{}》没有版本「{reference}」（数字已超出范围）",
                        parsed.title
                    ))
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
/// 现有的特殊页面。不在这里面的 `special:` 地址直接报「不存在」。
pub(crate) const SPECIAL_PAGES: [&str; 6] =
    ["newtab", "settings", "all", "random", "gc", "trash"];

/// 重定向最多跟几跳。超过就报错，而不是让 A→B→A 这类环无限递归。
///
/// 这是**仓库的跟跳策略**，不是指令语法 —— 语法在 [`crate::command`]。
const MAX_REDIRECT_HOPS: usize = 8;

/// 一条指令把这一页指向了哪里。
struct Chase {
    target: String,
    /// 是否随机跳转（决定提示语是"重定向自 X"还是"来自随机重定向"）
    random: bool,
}

/// 把仓库的随机能力交给指令表：指令只知道"要随机挑一篇"，怎么挑是这里的事。
impl crate::command::CommandEnv for Vault {
    fn random_title(&self, namespace: Option<&str>, from: &str) -> Result<String, String> {
        self.pick_random_title(namespace, from)
            .map_err(|error| error.to_string())
    }
}

/// 解析 RFC3339 时间。坏数据一律当作"解析不出"，由调用方决定怎么办。
fn parse_iso(text: &str) -> Option<time::OffsetDateTime> {
    time::OffsetDateTime::parse(text, &time::format_description::well_known::Rfc3339).ok()
}

/// 一篇笔记的删除时间：取最后一条删除标记的时间。
fn deletion_time(events: &[Event]) -> Option<time::OffsetDateTime> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::Del { at, .. } => parse_iso(at),
            _ => None,
        })
        .last()
}

/// 从某个时刻到现在过了多少天（未来时间当作 0 天，不算"过期"）。
fn days_since(at: time::OffsetDateTime) -> i64 {
    (time::OffsetDateTime::now_utc() - at).whole_days().max(0)
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
            Address::Note { title, address, .. } => {
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
            Address::Special { page, address, .. } => {
                assert_eq!(page, "newtab");
                assert_eq!(address, "special:newtab");
            }
            other => panic!("{other:?}"),
        }

        // 大小写不敏感，回显一律小写
        match temp.vault.parse_address("Special:NewTab").unwrap() {
            Address::Special { page, address, .. } => {
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

        // 前缀判断本身也要能处理多字节（这里曾按字节切，直接 panic）。
        // 现在前缀解析走命名空间表，所以用**行为**来断言，而不是测某个内部函数。
        assert!(
            matches!(
                temp.vault.parse_address("Special:newtab").unwrap(),
                Address::Special { .. }
            ),
            "特殊页面前缀大小写不敏感"
        );
        assert!(
            temp.vault.parse_address("特殊:newtab").is_err(),
            "没登记过的前缀不是命名空间"
        );
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
            Address::Note { address, title, .. } => {
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

    /// 特殊页面：不存在要报错，状态不正确要裁掉（章节与状态都按标准顺序回显）
    #[test]
    fn special_pages_validate_and_crop_state() {
        let temp = TempVault::new();

        // 状态不合法 → 裁掉，仍然打开那个页面（不当错误）
        match temp.vault.parse_address("special:newtab@edit").unwrap() {
            Address::Special { page, address, .. } => {
                assert_eq!(page, "newtab");
                assert_eq!(address, "special:newtab");
            }
            other => panic!("{other:?}"),
        }
        match temp.vault.parse_address("special:newtab@whatever").unwrap() {
            Address::Special { address, .. } => assert_eq!(address, "special:newtab"),
            other => panic!("{other:?}"),
        }

        // 章节保留
        match temp.vault.parse_address("special:newtab#小节").unwrap() {
            Address::Special { address, .. } => assert_eq!(address, "special:newtab#小节"),
            other => panic!("{other:?}"),
        }

        // 不存在的特殊页面 → 报错，且提示里带上现有的页面
        let error = temp.vault.parse_address("special:不存在").unwrap_err();
        let text = error.to_string();
        assert!(text.contains("没有这个特殊页面"), "{text}");
        assert!(text.contains("newtab"), "{text}");

        // 新增的页面同样注册在册（special:all）
        match temp.vault.parse_address("special:all").unwrap() {
            Address::Special { page, address, .. } => {
                assert_eq!(page, "all");
                assert_eq!(address, "special:all");
            }
            other => panic!("{other:?}"),
        }

        // 空页面名仍然是另一种错误
        assert!(temp.vault.parse_address("special:").is_err());
    }

    /// 界面偏好住在 preferences.json，且**不会**把仓库设置文件写脏
    #[test]
    fn appearance_lives_in_its_own_file() {
        let mut temp = TempVault::new();
        let before = fs::read_to_string(temp.root.join("vault.json")).unwrap();

        temp.vault
            .update_settings(
                None,
                None,
                None,
                None,
                None,
                Some("light".to_string()),
                Some("#123456".to_string()),
                Some(900),
                None,
            )
            .unwrap();

        let preferences = fs::read_to_string(temp.root.join("preferences.json")).unwrap();
        assert!(preferences.contains("\"light\""), "{preferences}");
        assert!(preferences.contains("#123456"), "{preferences}");
        assert!(preferences.contains("900"), "{preferences}");

        let after = fs::read_to_string(temp.root.join("vault.json")).unwrap();
        assert_eq!(before, after, "改外观不该动 vault.json");
        assert!(!after.contains("appearance"), "vault.json 里不该再有 appearance：{after}");

        assert_eq!(temp.vault.preferences().theme, "light");
        assert_eq!(temp.vault.settings_view().accent, "#123456");
    }

    /// 引用解析不出来时，提示里不能出现"没有版本 0"
    /// （0 是"给不出版本号"的内部占位，不是真实版本）
    #[test]
    fn unresolvable_reference_does_not_report_version_zero() {
        let temp = TempVault::new();
        temp.vault.create("引用").unwrap();
        temp.vault.commit("引用", "正文", None, 0).unwrap();

        // 5 位以上、既不是任何 commit 的前缀，也不是数字
        let error = temp.vault.resolve_revision("引用", "zzzzz").unwrap_err();
        let text = error.to_string();
        assert!(!text.contains("版本 0"), "不该打出占位用的 0：{text}");
        assert!(text.contains("没有这个版本"), "{text}");
    }

    /// 指令页面：第一行 `$$COMMAND$$`（忽略末尾空白）+ 第二行 `REDIRECT: 地址`
    #[test]
    fn command_page_redirects_and_no_command_shows_itself() {
        let temp = TempVault::new();
        temp.vault.create("目标").unwrap();
        temp.vault.commit("目标", "正文", None, 0).unwrap();

        // 第一行末尾故意留空格：规则是"忽略末尾空白"
        temp.vault.create("指令页").unwrap();
        temp.vault
            .commit("指令页", "$$COMMAND$$   \nREDIRECT: 目标\n", None, 0)
            .unwrap();

        // 不带状态：跟重定向，落到目标上（回显也是目标）
        match temp.vault.parse_address("指令页").unwrap() {
            Address::Note {
                title,
                address,
                code_block,
                via,
            } => {
                assert_eq!(title, "目标");
                assert_eq!(address, "目标");
                assert!(!code_block);
                // 跟重定向来的要带上"从哪儿来"：标题下方据此提示
                let via = via.expect("跟重定向来的应当有来源");
                assert_eq!(via.from, "指令页");
                assert!(!via.random, "这是重定向，不是随机跳转");
            }
            other => panic!("{other:?}"),
        }

        // @no-command：不跟重定向，显示它自己，回显保留状态
        match temp.vault.parse_address("指令页@no-command").unwrap() {
            Address::Note {
                title,
                address,
                code_block,
                via,
            } => {
                assert_eq!(title, "指令页");
                assert_eq!(address, "指令页@no-command");
                assert!(code_block);
                assert!(via.is_none(), "直接打开 @no-command 没有来源");
            }
            other => panic!("{other:?}"),
        }

        // 一般页面上的 @no-command：没有影响，回显裁掉
        match temp.vault.parse_address("目标@no-command").unwrap() {
            Address::Note {
                address, code_block, ..
            } => {
                assert_eq!(address, "目标");
                assert!(!code_block);
            }
            other => panic!("{other:?}"),
        }

        // @no-command 读出来的 html 是代码块，markdown 保持原样（编辑器里仍看到原文）
        let outcome = temp.vault.load_code_blocked("指令页").unwrap();
        let note = outcome.note.unwrap();
        assert!(note.html.contains("<pre"), "{}", note.html);
        assert!(note.html.contains("<code"), "{}", note.html);
        assert_eq!(note.markdown, "$$COMMAND$$   \nREDIRECT: 目标\n");
    }

    /// 指令页面认不出指令时**必须报错**，不能当普通页面读 ——
    /// 否则一条写坏的指令会静静显示成正文，谁也不知道它没生效。
    #[test]
    fn unrecognized_command_is_an_error() {
        let temp = TempVault::new();

        // 只有标记、没有第二行
        temp.vault.create("空指令").unwrap();
        temp.vault
            .commit("空指令", "$$COMMAND$$\n", None, 0)
            .unwrap();
        let error = temp.vault.parse_address("空指令").unwrap_err();
        assert!(error.to_string().contains("没写指令"), "{error}");

        // 有第二行，但不是 REDIRECT
        temp.vault.create("写错了").unwrap();
        temp.vault
            .commit("写错了", "$$COMMAND$$\nREDIRECTX: 目标\n", None, 0)
            .unwrap();
        let error = temp.vault.parse_address("写错了").unwrap_err();
        let text = error.to_string();
        assert!(text.contains("认不出来"), "{text}");
        assert!(text.contains("REDIRECTX"), "报错要带出写坏的那一行：{text}");

        // 逃生口：两者都能用 @no-command 进去看和改
        match temp.vault.parse_address("空指令@no-command").unwrap() {
            Address::Note { code_block, .. } => assert!(code_block),
            other => panic!("{other:?}"),
        }
        match temp.vault.parse_address("写错了@no-command").unwrap() {
            Address::Note { code_block, .. } => assert!(code_block),
            other => panic!("{other:?}"),
        }
    }

    /// 重定向成环要报错，而不是无限递归
    #[test]
    fn redirect_loop_is_reported() {
        let temp = TempVault::new();
        temp.vault.create("甲").unwrap();
        temp.vault
            .commit("甲", "$$COMMAND$$\nREDIRECT: 乙\n", None, 0)
            .unwrap();
        temp.vault.create("乙").unwrap();
        temp.vault
            .commit("乙", "$$COMMAND$$\nREDIRECT: 甲\n", None, 0)
            .unwrap();

        let error = temp.vault.parse_address("甲").unwrap_err();
        assert!(error.to_string().contains("重定向"), "{error}");
    }

    /// REDIRECT 没写目标 → 明确报错；而 @no-command 仍是逃生口，能进去看/改
    #[test]
    fn redirect_without_target_is_an_error() {
        let temp = TempVault::new();
        temp.vault.create("缺目标").unwrap();
        temp.vault
            .commit("缺目标", "$$COMMAND$$\nREDIRECT:\n", None, 0)
            .unwrap();

        assert!(temp.vault.parse_address("缺目标").is_err());
        match temp.vault.parse_address("缺目标@no-command").unwrap() {
            Address::Note { code_block, .. } => assert!(code_block),
            other => panic!("{other:?}"),
        }
    }

    /// 第一行不是标记时，即使第二行写了 REDIRECT 也不当指令页面
    #[test]
    fn redirect_needs_the_marker_on_the_first_line() {
        let temp = TempVault::new();
        temp.vault.create("目标2").unwrap();
        temp.vault.commit("目标2", "正文", None, 0).unwrap();
        temp.vault.create("伪装").unwrap();
        temp.vault
            .commit("伪装", "前言\nREDIRECT: 目标2\n", None, 0)
            .unwrap();

        match temp.vault.parse_address("伪装").unwrap() {
            Address::Note { address, .. } => assert_eq!(address, "伪装"),
            other => panic!("{other:?}"),
        }
    }

    /// `@view-<旧版>` 等同于启用 `@no-command`：看旧版时**不执行指令**，只把当时的原文
    /// 包成代码块显示。一般页面的版本视图不受影响。
    #[test]
    fn version_view_of_a_command_page_shows_code() {
        let temp = TempVault::new();
        temp.vault.create("目标").unwrap();
        temp.vault.commit("目标", "正文", None, 0).unwrap();
        temp.vault.create("指令").unwrap();
        temp.vault
            .commit("指令", "$$COMMAND$$\nREDIRECT: 目标\n", None, 0)
            .unwrap();

        // 直接看：跟重定向
        match temp.vault.parse_address("指令").unwrap() {
            Address::Note { title, .. } => assert_eq!(title, "目标"),
            other => panic!("{other:?}"),
        }

        // 看这一版：不跟，包成代码块；markdown 保持原文
        let content = temp.vault.revision_code_blocked("指令", 1).unwrap();
        assert!(content.html.contains("<pre"), "{}", content.html);
        assert_eq!(content.markdown, "$$COMMAND$$\nREDIRECT: 目标\n");

        // 一般页面的版本视图不该被包成代码块
        let plain = temp.vault.revision_code_blocked("目标", 1).unwrap();
        assert!(!plain.html.contains("<pre"), "{}", plain.html);
    }

    /// RANDOM_REDIRECT：在命名空间里随机挑一篇，并**排除自己**
    #[test]
    fn random_redirect_picks_another_page() {
        let temp = TempVault::new();
        temp.vault.create("唯一候选").unwrap();
        temp.vault.commit("唯一候选", "正文", None, 0).unwrap();
        temp.vault.create("掷骰子").unwrap();
        temp.vault
            .commit("掷骰子", "$$COMMAND$$\nRANDOM_REDIRECT\n", None, 0)
            .unwrap();

        // 候选只有一篇，所以结果必然确定（顺带证明"排除自己"生效：
        // 否则可能随机到自己，一路跟到跳数上限）
        match temp.vault.parse_address("掷骰子").unwrap() {
            Address::Note { title, .. } => assert_eq!(title, "唯一候选"),
            other => panic!("{other:?}"),
        }
    }

    /// 命名空间参数：写 ID 与"冒号后空着"都合法（空 = 主命名空间）
    #[test]
    fn random_redirect_accepts_namespace_argument() {
        let with_id = TempVault::new();
        with_id.vault.create("候选").unwrap();
        with_id.vault.commit("候选", "正文", None, 0).unwrap();
        with_id.vault.create("带ID").unwrap();
        with_id
            .vault
            .commit("带ID", "$$COMMAND$$\nRANDOM_REDIRECT: 0\n", None, 0)
            .unwrap();
        match with_id.vault.parse_address("带ID").unwrap() {
            Address::Note { title, .. } => assert_eq!(title, "候选"),
            other => panic!("{other:?}"),
        }

        let empty_arg = TempVault::new();
        empty_arg.vault.create("候选").unwrap();
        empty_arg.vault.commit("候选", "正文", None, 0).unwrap();
        empty_arg.vault.create("空参数").unwrap();
        empty_arg
            .vault
            .commit("空参数", "$$COMMAND$$\nRANDOM_REDIRECT: \n", None, 0)
            .unwrap();
        match empty_arg.vault.parse_address("空参数").unwrap() {
            Address::Note { title, .. } => assert_eq!(title, "候选"),
            other => panic!("{other:?}"),
        }
    }

    /// RANDOM_REDIRECT 的三种失败都要说清楚
    #[test]
    fn random_redirect_errors_are_clear() {
        // 唯一一页就是它自己 → 排除自己后没有候选
        let lonely = TempVault::new();
        lonely.vault.create("孤零零").unwrap();
        lonely
            .vault
            .commit("孤零零", "$$COMMAND$$\nRANDOM_REDIRECT\n", None, 0)
            .unwrap();
        let error = lonely.vault.parse_address("孤零零").unwrap_err();
        assert!(error.to_string().contains("随机不到"), "{error}");

        // 命名空间写成**字符串标识**：`special` 是虚拟命名空间，页面由程序提供
        let special = TempVault::new();
        special.vault.create("随便一篇").unwrap();
        special.vault.commit("随便一篇", "正文", None, 0).unwrap();
        special.vault.create("跳特殊页").unwrap();
        special
            .vault
            .commit("跳特殊页", "$$COMMAND$$\nRANDOM_REDIRECT: special\n", None, 0)
            .unwrap();
        match special.vault.parse_address("跳特殊页").unwrap() {
            Address::Special { page, .. } => assert!(
                SPECIAL_PAGES.contains(&page.as_str()),
                "应当落到真实存在的特殊页面：{page}"
            ),
            // 也可能正好抽中 `special:random` —— 它自己还会再跳一次，于是最终落在
            // 某一篇笔记上。这是正确行为（它就是"随机"），不是失败。
            Address::Note { .. } => {}
            other => panic!("{other:?}"),
        }

        // 命名空间里没有笔记
        let empty_ns = TempVault::new();
        empty_ns.vault.create("候选").unwrap();
        empty_ns.vault.commit("候选", "正文", None, 0).unwrap();
        empty_ns.vault.create("别的空间").unwrap();
        empty_ns
            .vault
            .commit("别的空间", "$$COMMAND$$\nRANDOM_REDIRECT: 7\n", None, 0)
            .unwrap();
        let error = empty_ns.vault.parse_address("别的空间").unwrap_err();
        assert!(error.to_string().contains("随机不到"), "{error}");
    }

    /// `special:random` 等同于随机重定向：落到主命名空间的某一篇
    #[test]
    fn special_random_jumps_to_a_page() {
        let temp = TempVault::new();
        temp.vault.create("唯一页").unwrap();
        temp.vault.commit("唯一页", "正文", None, 0).unwrap();

        match temp.vault.parse_address("special:random").unwrap() {
            Address::Note { title, .. } => assert_eq!(title, "唯一页"),
            other => panic!("{other:?}"),
        }

        // 一篇都没有 → 明确报错，而不是给一页空白
        let empty = TempVault::new();
        let error = empty.vault.parse_address("special:random").unwrap_err();
        assert!(error.to_string().contains("随机不到"), "{error}");
    }

    /// `special:all` 的列表要能看出哪些是指令页面（含认不出的那种）
    #[test]
    fn listing_marks_command_pages() {
        let temp = TempVault::new();
        temp.vault.create("目标").unwrap();
        temp.vault.commit("目标", "正文", None, 0).unwrap();
        temp.vault.create("重定向页").unwrap();
        temp.vault
            .commit("重定向页", "$$COMMAND$$\nREDIRECT: 目标\n", None, 0)
            .unwrap();
        temp.vault.create("随机页").unwrap();
        temp.vault
            .commit("随机页", "$$COMMAND$$\nRANDOM_REDIRECT\n", None, 0)
            .unwrap();
        temp.vault.create("坏指令").unwrap();
        temp.vault
            .commit("坏指令", "$$COMMAND$$\n不知道写什么\n", None, 0)
            .unwrap();

        let listed = temp.vault.list_notes().unwrap();
        let info_of = |title: &str| {
            listed
                .iter()
                .find(|item| item.title == title)
                .unwrap_or_else(|| panic!("列表里没有《{title}》"))
                .command
                .clone()
        };

        assert!(info_of("目标").is_none(), "普通页面没有指令信息");

        // 短名、中文名、说明**全部来自后端那张指令表** —— 前端不再自己维护一份清单
        let redirect = info_of("重定向页").expect("重定向页应当有指令信息");
        assert_eq!(redirect.kind, "redirect");
        assert_eq!(redirect.label, "重定向");
        assert!(redirect.detail.contains("重定向到"), "{}", redirect.detail);

        assert_eq!(info_of("随机页").unwrap().kind, "random-redirect");

        // 认不出的那种最需要被看见：一打开就报错，得先在列表里找到它
        let broken = info_of("坏指令").unwrap();
        assert_eq!(broken.kind, "unrecognized");
        assert_eq!(broken.label, "指令有问题");
    }

    /// 回收站清单：删过的笔记按删除时间倒序列出，并带上"删了多久"
    #[test]
    fn trash_listing_reports_age() {
        let temp = TempVault::new();
        temp.vault.create("留下的").unwrap();
        temp.vault.commit("留下的", "正文", None, 0).unwrap();
        temp.vault.create("删掉的").unwrap();
        temp.vault.commit("删掉的", "被删的正文", None, 0).unwrap();
        temp.vault.delete("删掉的").unwrap();

        let listed = temp.vault.list_trash().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].title, "删掉的");
        assert_eq!(listed[0].days_old, Some(0), "刚删的应当是 0 天");
        assert!(listed[0].bytes > 0);
        assert!(!listed[0].deleted_at.is_empty());
    }

    /// 清理回收站：30 天不动刚删的；传 0 则全清，并顺手回收内容块
    #[test]
    fn purge_trash_respects_age_then_reclaims_blobs() {
        let temp = TempVault::new();
        temp.vault.create("留下的").unwrap();
        temp.vault.commit("留下的", "留下的正文", None, 0).unwrap();
        temp.vault.create("删掉的").unwrap();
        temp.vault
            .commit("删掉的", "只此一份的正文", None, 0)
            .unwrap();
        temp.vault.delete("删掉的").unwrap();

        // 30 天：刚删的不该被动
        let report = temp.vault.purge_trash(30).unwrap();
        assert_eq!(report.removed, 0);
        assert_eq!(temp.vault.list_trash().unwrap().len(), 1, "刚删的必须留着");

        // 0 天：全部清掉；那份正文只被这一页引用，所以内容块这时才成为孤块
        let report = temp.vault.purge_trash(0).unwrap();
        assert_eq!(report.removed, 1);
        assert!(temp.vault.list_trash().unwrap().is_empty());
        assert_eq!(
            report.blobs.removed_blobs, 1,
            "回收站日志没了，那份正文才无人引用"
        );
        assert!(temp.vault.load("留下的").is_ok(), "没删的那篇不受影响");
    }

    /// 回收站里的引用要**连同草稿节点**一起计入 blob 统计。
    ///
    /// 已有的用例只覆盖了提交版本（`Rev`）；这里补上草稿（`Auto`）那一半 ——
    /// 只被草稿引用的内容块，同样不能因为"引用它的笔记被删了"就被回收。
    #[test]
    fn gc_counts_draft_references_from_the_trash() {
        let temp = TempVault::new();
        temp.vault.create("带草稿").unwrap();
        temp.vault.commit("带草稿", "第一版", None, 0).unwrap();
        // 一份只属于草稿的正文
        temp.vault.save_draft("带草稿", "草稿独有的正文", 1).unwrap();
        temp.vault.delete("带草稿").unwrap();

        let report = temp.vault.gc(true, false).unwrap();
        assert_eq!(report.removed_blobs, 0, "回收站里的草稿仍然引用着它的内容块");

        // 整条清掉回收站之后，这块才真正无人引用
        let purged = temp.vault.purge_trash(0).unwrap();
        assert_eq!(purged.removed, 1);
        assert!(
            purged.blobs.removed_blobs >= 1,
            "回收站清掉后，草稿独有的内容块才成为孤块"
        );
    }

    /// "到点了吗"：刚跑过就不是，间隔过去或从没跑过就是
    #[test]
    fn maintenance_pending_follows_the_last_run() {
        let fresh = TempVault::new();
        assert!(fresh.vault.maintenance_pending(), "从没跑过就该跑");

        let mut ran = TempVault::new();
        ran.vault.run_maintenance().unwrap();
        assert!(
            !ran.vault.maintenance_pending(),
            "刚跑过、间隔没到，就不该再建任务"
        );
    }

    /// 还原：文件搬回来、名字挪回去，并**追加一版**（历史一条不丢）
    #[test]
    fn restore_brings_a_note_back() {
        let temp = TempVault::new();
        temp.vault.create("回来的").unwrap();
        temp.vault.commit("回来的", "正文还在", None, 0).unwrap();
        temp.vault.delete("回来的").unwrap();
        assert!(temp.vault.list_trash().unwrap().len() == 1);

        let note = temp.vault.restore_note("回来的").unwrap();
        assert_eq!(note.markdown, "正文还在", "内容原样回来");
        assert!(temp.vault.list_trash().unwrap().is_empty());
        assert!(
            temp.vault.list_notes().unwrap().iter().any(|item| item.title == "回来的"),
            "应当重新出现在笔记列表里"
        );
        assert!(temp.vault.load("回来的").is_ok());

        // 历史一条不丢：创建 / 提交 / 删除 / 还原
        let history = temp.vault.history("回来的").unwrap();
        assert!(
            history.iter().any(|item| item.summary.as_deref() == Some("从回收站还原")),
            "还原本身应当在历史里留下一笔"
        );
        assert!(history.len() >= 4, "{history:?}");
    }

    /// 立即清除：只清一条，别的还在；且**不**顺手回收内容块（那件事交给数据库回收页）
    #[test]
    fn purge_entry_removes_one() {
        let temp = TempVault::new();
        for title in ["留着", "马上清"] {
            temp.vault.create(title).unwrap();
            temp.vault.commit(title, "各自的内容", None, 0).unwrap();
            temp.vault.delete(title).unwrap();
        }

        temp.vault.purge_trash_entry("马上清").unwrap();
        let listed = temp.vault.list_trash().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].title, "留着");

        assert!(temp.vault.restore_note("留着").is_ok(), "别的那条仍可还原");
        assert!(temp.vault.purge_trash_entry("马上清").is_err(), "已经清了");
    }

    /// 自动维护：**按上次执行时间判定**，间隔之内什么都不做
    #[test]
    fn maintenance_runs_once_per_interval() {
        let mut temp = TempVault::new();
        temp.vault.create("甲").unwrap();
        temp.vault.commit("甲", "正文", None, 0).unwrap();
        temp.vault.create("乙").unwrap();
        temp.vault.commit("乙", "待清理的正文", None, 0).unwrap();
        // 传 0 天：让它这次就到点
        temp.vault
            .update_settings(None, None, None, Some(0), Some(0), None, None, None, None)
            .unwrap();
        temp.vault.delete("乙").unwrap();

        // 第一次：从没跑过 → 该跑
        let first = temp.vault.run_maintenance().unwrap();
        assert!(first.purged.is_some(), "从没跑过就该跑一次");
        assert!(first.gc.is_some());
        assert_eq!(
            temp.vault.list_trash().unwrap().len(),
            1,
            "保留期下限是 1 天：刚删的那条不该被自动清掉"
        );

        // 再跑一次：刚跑过，间隔没到 → 什么都不做
        let second = temp.vault.run_maintenance().unwrap();
        assert!(second.purged.is_none(), "间隔之内不该重复清");
        assert!(second.gc.is_none());

        // 判定靠"上次执行时间"，所以它必须被记下来
        let view = temp.vault.settings_view();
        assert!(!view.last_trash_purge.is_empty(), "上次清理时间要写回去");
        assert!(!view.last_gc.is_empty(), "上次回收时间要写回去");
    }

    /// 重定向到**特殊页面**时同样带上来源：虚拟命名空间下的页面也是"页面"，
    /// 用户同样需要知道自己是**被带过来的**。
    #[test]
    fn redirect_to_special_page_carries_the_source() {
        let temp = TempVault::new();
        temp.vault.create("跳到设置").unwrap();
        temp.vault
            .commit(
                "跳到设置",
                "$$COMMAND$$\nREDIRECT: special:settings\n",
                None,
                0,
            )
            .unwrap();

        match temp.vault.parse_address("跳到设置").unwrap() {
            Address::Special { page, via, .. } => {
                assert_eq!(page, "settings");
                let via = via.expect("重定向过来的应当有来源");
                assert_eq!(via.from, "跳到设置");
                assert!(!via.random, "这是重定向，不是随机跳转");
            }
            other => panic!("{other:?}"),
        }
    }

    /// 命名空间：名称与别名都不许重复，保留名不许占用
    #[test]
    fn namespace_names_must_be_unique() {
        let mut temp = TempVault::new();
        temp.vault
            .add_namespace("help", vec!["帮助".to_string()], None)
            .unwrap();

        assert!(temp.vault.add_namespace("help", Vec::new(), None).is_err());
        assert!(temp.vault.add_namespace("HELP", Vec::new(), None).is_err());
        assert!(temp
            .vault
            .add_namespace("other", vec![" help ".to_string()], None)
            .is_err());
        assert!(temp
            .vault
            .add_namespace("other", vec!["甲".to_string(), "甲".to_string()], None)
            .is_err());
        assert!(temp.vault.add_namespace("special", Vec::new(), None).is_err());
        assert!(temp.vault.add_namespace("0", Vec::new(), None).is_err());
        assert!(temp.vault.add_namespace("  ", Vec::new(), None).is_err());

        // 别名解析成规范名（链接回显走的就是这条路）
        temp.vault.create("help:入门").unwrap();
        temp.vault.commit("help:入门", "正文", None, 0).unwrap();
        match temp.vault.parse_address("帮助:入门").unwrap() {
            Address::Note { title, .. } => assert_eq!(title, "help:入门"),
            other => panic!("{other:?}"),
        }
    }

    /// 两跳寻址：目录用**标识**，名字只用于显示与匹配
    #[test]
    fn namespaces_use_a_stable_id() {
        let mut temp = TempVault::new();
        temp.vault
            .add_namespace("help", vec!["帮助".to_string()], None)
            .unwrap();

        let id = temp
            .vault
            .namespaces()
            .into_iter()
            .find(|item| item.name == "help")
            .expect("新命名空间应当在表里")
            .id;
        assert_ne!(id, "help", "标识是生成的，不该等于名字");

        for title in ["help:甲", "help:乙"] {
            temp.vault.create(title).unwrap();
            temp.vault.commit(title, "正文", None, 0).unwrap();
        }
        assert!(
            temp.root.join("notes").join(&id).is_dir(),
            "笔记应当落在以**标识**命名的目录里"
        );
        assert_eq!(temp.vault.load("help:甲").unwrap().title, "help:甲");

        // 别名认得出同一页
        match temp.vault.parse_address("帮助:甲").unwrap() {
            Address::Note { title, .. } => assert_eq!(title, "help:甲"),
            other => panic!("{other:?}"),
        }

        // 清空：按**名字**调用，内部换成标识；页面进回收站，命名空间还在
        assert_eq!(temp.vault.empty_namespace("help").unwrap(), 2);
        assert_eq!(temp.vault.list_trash().unwrap().len(), 2);

        // 删除（已清空）：条目消失；还原明确报"命名空间不存在"
        assert_eq!(temp.vault.delete_namespace("help").unwrap(), 0);
        let error = temp.vault.restore_note("help:甲").unwrap_err();
        assert!(error.to_string().contains("命名空间"), "{error}");

        // 但那条记录必须还能被清掉（命名空间没了，只能按 id 找），否则永久卡在回收站里
        temp.vault.purge_trash_entry("help:甲").unwrap();
        assert_eq!(temp.vault.list_trash().unwrap().len(), 1);

        // 保留名不可删
        assert!(temp.vault.delete_namespace("special").is_err());
        assert!(temp.vault.delete_namespace("0").is_err());
    }

    /// 改名只改表里一行：**文件一个都不搬**，链接跟着新名字走
    #[test]
    fn renaming_a_namespace_moves_nothing() {
        fn dirs(root: &Path) -> Vec<String> {
            let mut out: Vec<String> = fs::read_dir(root.join("notes"))
                .unwrap()
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().to_string())
                .collect();
            out.sort();
            out
        }

        let mut temp = TempVault::new();
        temp.vault.add_namespace("help", Vec::new(), None).unwrap();
        temp.vault.create("help:甲").unwrap();
        temp.vault.commit("help:甲", "正文", None, 0).unwrap();

        let before = dirs(&temp.root);
        temp.vault.rename_namespace("help", "帮助").unwrap();
        assert_eq!(before, dirs(&temp.root), "改名不该动任何目录或文件");

        // 新名字认识它，旧名字不再认识
        assert_eq!(temp.vault.load("帮助:甲").unwrap().title, "帮助:甲");
        assert!(temp.vault.load("help:甲").is_err(), "旧名字已经不属于它了");

        // 旧名字可以给别人用（键是标识，不是名字）
        temp.vault.add_namespace("help", Vec::new(), None).unwrap();
        // 保留名不能改名
        assert!(temp.vault.rename_namespace("special", "别的").is_err());
    }

    /// 跨站链接：前缀配了站点地址 → 渲染成带 href 的绿链，不在本仓库里查页面
    #[test]
    fn interwiki_links_point_at_another_site() {
        let mut temp = TempVault::new();
        temp.vault
            .add_namespace(
                "zhwiki",
                Vec::new(),
                Some("https://zh.wikipedia.org/wiki/$1".to_string()),
            )
            .unwrap();

        let item = temp
            .vault
            .namespaces()
            .into_iter()
            .find(|item| item.name == "zhwiki")
            .unwrap();
        assert!(!item.storable, "配了站点的命名空间页面在别处，本仓库不存");
        assert_eq!(
            item.url_for("New York").unwrap(),
            "https://zh.wikipedia.org/wiki/New_York",
            "空格按 MediaWiki 习惯折成下划线"
        );

        temp.vault.create("引用").unwrap();
        temp.vault
            .commit("引用", "看 [[zhwiki:NASA|NASA]]。", None, 0)
            .unwrap();

        let html = temp.vault.load("引用").unwrap().html;
        assert!(
            html.contains(r#"href="https://zh.wikipedia.org/wiki/NASA""#),
            "{html}"
        );
        assert!(html.contains(r#"data-interwiki="true""#), "{html}");
        assert!(
            !html.contains("data-missing"),
            "跨站链接不该被判成红链（那是本仓库有没有这一页的事）：{html}"
        );
    }

    /// 别名可增可减；保留的两个也能配别名（special 的别名要真的路由过去）；
    /// 主命名空间可以清空
    #[test]
    fn aliases_are_editable_everywhere() {
        let mut temp = TempVault::new();
        temp.vault.add_namespace("help", Vec::new(), None).unwrap();
        temp.vault.create("help:条目").unwrap();
        temp.vault.commit("help:条目", "正文", None, 0).unwrap();

        // 一次给两个别名，两个都认
        temp.vault
            .update_namespace_aliases("help", vec!["帮助".to_string(), "百科".to_string()])
            .unwrap();
        for alias in ["帮助", "百科"] {
            let address = format!("{alias}:条目");
            match temp.vault.parse_address(&address).unwrap() {
                Address::Note { title, .. } => assert_eq!(title, "help:条目"),
                other => panic!("{address} → {other:?}"),
            }
        }

        // 删到一个：去掉的那个不再认
        temp.vault
            .update_namespace_aliases("help", vec!["帮助".to_string()])
            .unwrap();
        assert!(temp.vault.parse_address("百科:条目").is_err(), "别名已删掉");

        // 别名之间不许重复（大小写与空白不影响判重）
        assert!(temp
            .vault
            .update_namespace_aliases("help", vec!["帮助".to_string(), " 帮助 ".to_string()])
            .is_err());

        // special 的别名要真的路由到特殊页面
        temp.vault
            .update_namespace_aliases("special", vec!["特殊".to_string()])
            .unwrap();
        match temp.vault.parse_address("特殊:gc").unwrap() {
            Address::Special { page, .. } => assert_eq!(page, "gc"),
            other => panic!("{other:?}"),
        }

        // 主命名空间也能配别名：`主:某页` 落在主命名空间。
        // 没建过那一页时应当是"缺失"，而不是报标题非法 —— 这说明前缀被正确认成了
        // 主命名空间（主命名空间没有前缀，所以显示标题里看不到它）。
        temp.vault
            .update_namespace_aliases("0", vec!["主".to_string()])
            .unwrap();
        match temp.vault.parse_address("主:不存在的页").unwrap() {
            Address::Missing { title, .. } => assert_eq!(title, "不存在的页"),
            other => panic!("{other:?}"),
        }
        temp.vault.create("主:另一页").unwrap();
        temp.vault.commit("主:另一页", "正文", None, 0).unwrap();
        match temp.vault.parse_address("主:另一页").unwrap() {
            Address::Note { title, .. } => assert_eq!(title, "另一页"),
            other => panic!("{other:?}"),
        }

        // 主命名空间可以**清空**（它只不能改名与删除）
        temp.vault.create("常规条目").unwrap();
        temp.vault.commit("常规条目", "正文", None, 0).unwrap();
        assert!(temp.vault.empty_namespace("0").unwrap() >= 1);
        assert!(
            !temp
                .vault
                .list_notes()
                .unwrap()
                .iter()
                .any(|item| item.title == "常规条目"),
            "清空之后主命名空间里不该还有它"
        );
        assert!(temp.vault.delete_namespace("0").is_err());
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
