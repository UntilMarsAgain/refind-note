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

mod debug;
mod namespaces;
mod trash;

pub use api::{
    Address, CommandInfo, DiffResult, Draft, GcReport, LoadOutcome, MaintenanceReport, Note,
    NoteSummary, PurgeReport, RevisionContent, RevisionSummary, TrashEntry, VaultSettings, Via,
    DebugReport, RenderReport,
};
pub use config::VaultConfig;
pub use error::VaultError;
pub use event::{
    drafts_of, fold, next_rev, revision_id, revision_of, short_revision_id, Event,
};

use atomic::{append_line, hash_bytes, write_atomic, BlobStore};
use std::collections::{BTreeMap, HashMap, HashSet};
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
        let mut table: NamespaceTable = match fs::read_to_string(&namespaces_path) {
            Ok(text) => serde_json::from_str(&text)?,
            Err(_) => {
                let table = NamespaceTable::builtin();
                write_atomic(&namespaces_path, &serde_json::to_vec_pretty(&table)?)?;
                table
            }
        };
        // 老仓库升级上来时补一次默认的跨站命名空间；播过之后就不再动，用户删掉的不会回来。
        // （模板命名空间不做升级补齐：需要它的仓库还一个都没有。）
        if table.sow_defaults() {
            write_atomic(&namespaces_path, &serde_json::to_vec_pretty(&table)?)?;
        }

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

    /// 构造渲染用的解析器：键集合来自目录遍历（没有全局索引可查）。
    ///
    /// 同一次遍历里顺带把**模板命名空间**的正文读进解析器：模板渲染器只能通过解析器取素材
    /// （渲染发生在 markdown-it 的回调里，那里没有仓库）。模板通常只有几个、都很小，
    /// 每次渲染读一遍不值得优化；真到了要优化的时候，再换成带失效的缓存。
    fn resolver(&self, from: Option<ParsedTitle>) -> LinkResolver {
        let mut keys: HashSet<String> = HashSet::new();
        let mut templates: HashMap<String, String> = HashMap::new();
        for parsed in self.walk(&self.notes_dir()).unwrap_or_default() {
            keys.insert(parsed.key());
            if parsed.ns == crate::title::TEMPLATE_NS {
                if let Ok(Some(text)) = self.current_markdown(&parsed.display(&self.table)) {
                    templates.insert(parsed.title.clone(), text);
                }
            }
        }

        LinkResolver::new(
            Arc::clone(&self.table),
            Arc::new(keys),
            self.config.capital_links,
            from,
        )
        .with_templates(Arc::new(templates))
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

    /// 解析器看到的模板页（调试用）
    pub fn template_names(&self) -> Vec<(String, usize)> {
        self.resolver(None).template_names()
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
        // 语言判定一次，html 与给前端的字段共用同一结果
        let language = crate::title::code_template_language(&parsed.ns, &parsed.title);
        let html = match language {
            // 模板命名空间里的 .css / .html 是**素材**，不是文档：当代码块显示
            Some(language) => markdown::render_with(
                &markdown::fence_code_in(&markdown_text, language),
                Some(&resolver),
            ),
            None => markdown::render_with(&markdown_text, Some(&resolver)),
        };

        Ok(LoadOutcome {
            note: Some(Note {
                key: parsed.key(),
                title: parsed.display(&self.table),
                markdown: markdown_text,
                html,
                language: language.map(str::to_string),
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
        let html = match crate::title::code_template_language(&parsed.ns, &parsed.title) {
            // 模板命名空间里的 .css / .html 是**素材**，不是文档：当代码块显示
            Some(language) => markdown::render_with(
                &markdown::fence_code_in(&markdown_text, language),
                Some(&resolver),
            ),
            None => markdown::render_with(&markdown_text, Some(&resolver)),
        };

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

    /// 跳数上限统一把关：会"跟下去"的指令都先过这一关，免得新增指令时漏掉。
    fn guard_hops(&self, title: &str, hops: usize) -> Result<(), VaultError> {
        if hops >= MAX_REDIRECT_HOPS {
            return Err(VaultError::BadAddress(format!(
                "重定向超过 {MAX_REDIRECT_HOPS} 跳，可能成环（停在 {title}）"
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
                "{from} 随机不到页面：{label} 里没有别的页面"
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
                    .map_err(|message| VaultError::BadAddress(format!("{title} {message}")))?
                {
                    Some(target) => Ok(Some(Chase { target, random })),
                    // 表里标明"不跳"的指令：当普通页面读
                    None => Ok(None),
                }
            }
            // 是指令页面，但指令本身有问题 —— **不能当普通页面读**：
            // 那样一条写坏的指令会静静显示成正文，谁也不知道它没生效。
            crate::command::Parsed::Empty => Err(VaultError::BadAddress(format!(
                "{title} 是指令页面，但没写指令（第二行应写成 {}）",
                crate::command::supported()
            ))),
            crate::command::Parsed::Unrecognized(line) => Err(VaultError::BadAddress(format!(
                "{title} 的指令认不出来：「{}」；目前支持 {}",
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
                "{} 的版本引用是空的",
                parsed.title
            )));
        }

        let digits = reference.chars().all(|ch| ch.is_ascii_digit());

        // 不足 5 位：只能当版本号。缩写按约定至少 5 位，太短给明确提示。
        if reference.len() < MIN_SHORT_ID {
            if digits {
                return reference.parse::<u64>().map_err(|_| {
                    VaultError::BadAddress(format!(
                        "{} 没有版本「{reference}」（数字已超出范围）",
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
        // 内容没变就不追加，免得自动保存把日志灌满。
        //
        // 两种"没变"都要挡：
        // - 与上一份草稿相同（自动保存按节流反复调用）；
        // - 与**最新提交的正文**相同 —— 提交之后草稿被取代，此时再存一次，就会凭空
        //   多出一个与正文一模一样的草稿，历史里看着就是一堆空版本。
        let unchanged = match &state.draft {
            Some(draft) => draft.blob == content_hash,
            None => {
                let committed = if state.rev == 0 {
                    String::new()
                } else {
                    self.content_of(&events, state.rev, 0)?
                };
                committed == markdown_text
            }
        };
        if unchanged {
            return Ok(());
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
            .ok_or_else(|| VaultError::Corrupt("找不到这份草稿".to_string()))?;
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
pub(crate) const SPECIAL_PAGES: [&str; 7] =
    ["newtab", "settings", "all", "random", "gc", "trash", "debug"];

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
mod tests;
