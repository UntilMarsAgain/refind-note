//! 给前端的结构。
//!
//! 单独放着，是为了让「对外形状」和「内部实现」分开：改内部不必动前端契约。

use serde::Serialize;

// ---------------------------------------------------------------- 对外结构

#[derive(Debug, Clone, Serialize)]
pub struct NoteSummary {
    /// 规范键，形如 `0:平陆运河`
    pub key: String,
    /// 显示标题
    pub title: String,
    /// 指令页面的短名（`redirect` / `random-redirect` / `unrecognized`）；
    /// 普通页面是 `null`。`special:all` 据此标注。
    pub command: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Note {
    pub key: String,
    /// 显示标题（含命名空间前缀）
    pub title: String,
    /// 编辑用的原样源码
    pub markdown: String,
    /// 阅读用的 HTML（Rust 端编译，含红/蓝链标记）
    pub html: String,
    pub rev: u64,
    pub modified: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Draft {
    pub markdown: String,
    pub base_rev: u64,
    pub at: String,
    /// 草稿也是链上的一版，所以同样有 ID；
    /// 界面据此用「标题@缩写」预览它，而不必单独做一套预览通道
    pub id: String,
    pub short_id: String,
}

/// `load_note` 的结果。
///
/// 刻意**不用 `Result` 表达「目标不存在」**：那不是错误，而是需要界面配合的正常
/// 状态（显示「还没有这篇笔记」并给一个创建按钮）。用错误表达，前端就分不清
/// 「不存在」和「真的读写失败」了。
#[derive(Debug, Clone, Serialize)]
pub struct LoadOutcome {
    /// 目标存在时的笔记内容；`None` 表示还不存在
    pub note: Option<Note>,
    /// 请求的显示标题（前端创建时直接拿去用）
    pub title: String,
    /// 是被删除过，而不是从未建立（区分开是为了将来接恢复功能）
    pub deleted: bool,
}

/// 历史的每一行：只有元信息，正文按需再取（见 `RevisionContent`）。
#[derive(Debug, Clone, Serialize)]
pub struct RevisionSummary {
    /// 0 表示「创建」这条记录，它本身没有正文
    pub rev: u64,
    /// create / commit / draft / delete
    pub kind: String,
    pub at: String,
    pub bytes: u64,
    /// 相对上一条记录的字节增减，便于一眼看出改了多少
    pub delta: i64,
    /// `full` 或 `delta`：这一版内容是怎么存的
    pub encoding: String,
    /// 完整 commit ID（界面上一般只显示缩写）
    pub id: String,
    /// 展示用的缩写
    pub short_id: String,
    /// 本次提交取代了哪些草稿节点（只有 commit 才有）
    pub supersedes: Vec<u64>,
    pub summary: Option<String>,
}

/// 某一个版本的正文
#[derive(Debug, Clone, Serialize)]
pub struct RevisionContent {
    pub rev: u64,
    pub kind: String,
    pub at: String,
    /// 该版本**当时**的显示标题：改名之前的版本显示旧标题
    pub title: String,
    pub markdown: String,
    pub html: String,
}

/// 对比结果里的一行
#[derive(Debug, Clone, Serialize)]
pub struct DiffLine {
    /// equal / insert / delete
    pub kind: String,
    /// 行号从 1 起；该侧没有这一行时是 null（插入的行没有旧行号）
    pub old_line: Option<u64>,
    pub new_line: Option<u64>,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiffResult {
    pub from_rev: u64,
    pub to_rev: u64,
    pub from_title: String,
    pub to_title: String,
    pub lines: Vec<DiffLine>,
    pub inserted: usize,
    pub deleted: usize,
}
/// 地址栏那一行的解析结果。
///
/// 后端**解析到底**：哪篇笔记、哪一版、还是不存在。前端只按 `kind` 分发 ——
/// 加新语法时在这里加变体，界面补一个分支即可。
///
/// 返回的 `title` / `short_id` 都是**规范全称**，界面直接拿来回显地址栏
/// （所以用 `@缩写` 跳转之后，地址栏会显示成 `名称@缩写`）。
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Address {
    /// 空输入
    Empty,
    /// 阅读一篇笔记（已确认存在）
    Note {
        title: String,
        address: String,
        /// 指令页面 + `@no-command`：正文要包成**代码块**显示（不执行指令）
        code_block: bool,
    },
    /// 编辑
    Edit { title: String, address: String },
    /// 版本历史（整篇）
    History { title: String, address: String },
    /// 删除的**二次确认页**
    Delete { title: String, address: String },
    /// 只读查看某一版（`view-版本`）
    ViewVersion {
        title: String,
        rev: u64,
        id: String,
        short_id: String,
        address: String,
    },
    /// 回退的**二次确认页**（`rollback-版本`）
    RollbackConfirm {
        title: String,
        rev: u64,
        id: String,
        short_id: String,
        address: String,
    },
    /// 特殊页面：**虚拟命名空间** `special:`。它不对应任何笔记文件，
    /// 由前端按 `page` 渲染（例如 `special:newtab`）。
    Special { page: String, address: String },
    /// 目标还不存在，交给「不存在 + 创建」那条路
    Missing { title: String, address: String },
}

/// 一次自动维护的结果。`None` 表示这一项这次**没到时间**，没跑。
#[derive(Debug, Default, Serialize)]
pub struct MaintenanceReport {
    pub purged: Option<PurgeReport>,
    pub gc: Option<GcReport>,
}

/// 回收站里的一个条目
#[derive(Debug, Clone, Serialize)]
pub struct TrashEntry {
    pub title: String,
    /// 删除时间（RFC3339，原样给出，界面自己决定怎么显示）
    pub deleted_at: String,
    /// 这份日志占用的字节数
    pub bytes: u64,
    /// 删了多少天（0 = 今天删的）。
    ///
    /// `None` 表示**删除时间读不出来**（日志里没有可解析的删除标记）。它与"年代久远"
    /// 是两件事：这种条目会被列出来让人看见，但**不参与清理** —— 宁可不删，不可误删。
    pub days_old: Option<i64>,
}

/// 一次回收站清理的结果
#[derive(Debug, Clone, Serialize)]
pub struct PurgeReport {
    /// 清掉了几条回收站记录
    pub removed: usize,
    /// 这些日志本身释放的字节
    pub freed_bytes: u64,
    /// 顺手做的那次内容块回收（这些笔记的内容块此时才真正无人引用）
    pub blobs: GcReport,
}

/// 一次回收的结果
#[derive(Debug, Clone, Default, Serialize)]
pub struct GcReport {
    /// 回收掉的悬置 blob 数
    pub removed_blobs: usize,
    /// 释放的字节数
    pub freed_bytes: u64,
    /// 清掉的、已被取代的草稿节点数
    pub removed_drafts: usize,
}

/// 仓库级设置。**界面偏好不在这里**——那属于这台机器，不属于数据，留在前端。
#[derive(Debug, Clone, Serialize)]
pub struct VaultSettings {
    /// 仓库根目录（界面上显示出来，方便直接去看文件）
    pub root: String,
    pub format: u32,
    /// 标题首字母是否强制大写（对应 MediaWiki 的 $wgCapitalLinks）
    pub capital_links: bool,
    pub max_title_bytes: usize,
    /// 增量链长度上限（0 之外的任何值都合法；界面里给个合理区间）
    pub delta_chain_limit: usize,
    /// 回收站保留天数（自动清理用）
    pub trash_keep_days: u64,
    /// 自动回收的间隔天数
    pub gc_interval_days: u64,
    /// 上次清理回收站的时间（只读信息）
    pub last_trash_purge: String,
    /// 上次回收的时间（只读信息）
    pub last_gc: String,
    pub theme: String,
    pub accent: String,
    pub reading_width: u32,
    /// 界面缩放（1.0 = 100%）
    pub zoom: f64,
}
