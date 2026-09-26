//! 给前端的结构。
//!
//! 单独放着，是为了让「对外形状」和「内部实现」分开：改内部不必动前端契约。

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------- 对外结构

#[derive(Debug, Clone, Serialize)]
pub struct NoteSummary {
    /// 规范键，形如 `0:平陆运河`
    pub key: String,
    /// 显示标题
    pub title: String,
    /// 指令信息（短名 / 中文名 / 一句说明）；普通页面是 `null`。
    /// `special:all` 据此标注，**不必在前端再维护一份命令清单**。
    pub command: Option<CommandInfo>,
}

/// 是通过哪条指令来到这一页的（跟过重定向的阅读路径会带上）。
#[derive(Debug, Clone, Serialize)]
pub struct Via {
    /// 来源页面标题；随机跳转时它是**发起随机的页面**，不是目标
    pub from: String,
    /// 是否随机跳转：提示语据此写成"来自随机重定向"
    pub random: bool,
}

/// 一条指令页面的对外信息。
///
/// 全部来自后端那张指令表（`command.rs`），前端只负责显示 —— 加新指令时前端不用改。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CommandInfo {
    /// 短名：`redirect` / `random-redirect` / `unrecognized`
    pub kind: String,
    /// 中文名，界面直接显示
    pub label: String,
    /// 一句人话说明（"打开这一页会跳到" / "认不出来：「…」"）
    pub detail: String,
    /// 指令的原文参数（重定向的目标地址 / 随机跳转的命名空间）；没有参数时为空。
    ///
    /// 单独给出来，是为了让界面能把它渲染成**可点的链接** —— 写进 `detail` 就只能当文字。
    pub argument: String,
}

impl CommandInfo {
    /// 从解析结果生成；不是指令页面就没有信息
    pub fn from_parsed(parsed: &crate::command::Parsed) -> Option<Self> {
        Some(CommandInfo {
            kind: parsed.kind()?.to_string(),
            label: parsed.label()?.to_string(),
            detail: parsed.describe(),
            argument: parsed
                .command()
                .map(|command| command.argument.clone())
                .unwrap_or_default(),
        })
    }
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
    /// 这一页该按什么语言对待：`css` / `html`（模板命名空间里的代码模板），
    /// 其余为 `null`（按 markdown）。编辑器据此选高亮规则 ——
    /// 判定只写在后端一处，前端不再自己认一遍后缀。
    pub language: Option<String>,
    pub rev: u64,
    pub modified: String,
}

/// 诊断报告里的一行：一个标签配一个值
#[derive(Debug, Clone, Serialize)]
pub struct DebugEntry {
    pub label: String,
    /// 值可以是多行（比如渲染出的 HTML）
    pub value: String,
}

/// 诊断报告里的一段
#[derive(Debug, Clone, Serialize)]
pub struct DebugSection {
    pub title: String,
    pub entries: Vec<DebugEntry>,
}

/// 编辑页「状态」面板要的编译报告：这次渲染到底发生了什么。
///
/// 与诊断页同一套事实来源（模板块探针、语言判定），只是输入换成编辑器里的**当前文本**。
#[derive(Debug, Clone, Serialize)]
pub struct RenderReport {
    /// 送进渲染器的文本规模
    pub markdown_bytes: usize,
    pub markdown_lines: usize,
    /// 渲染出的 HTML（面板里截断显示，复制时给全）
    pub html: String,
    pub html_bytes: usize,
    /// 渲染耗时（毫秒）
    pub millis: u64,
    /// 这一页按什么语言对待（`css` / `html` / markdown）
    pub language: String,
    /// 正文里每条模板块会走哪条分发路径
    pub blocks: Vec<DebugEntry>,
}

/// 诊断报告：仓库当前是什么样（`special:debug`）
///
/// 分段给出，是为了让它**可读**：一屏能扫完，而不是一段糊在一起的日志。
#[derive(Debug, Clone, Serialize)]
pub struct DebugReport {
    pub sections: Vec<DebugSection>,
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
///
/// **判别式只有这一处定义**：TypeScript 那一侧由 `ts-rs` 从这里导出
/// （`cargo test` 会写进 `src/bindings/`），前端不再手写第二份 —— 手写的那份已经漂移过。
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Address {
    /// 空输入
    Empty,
    /// 阅读一篇笔记（已确认存在）
    Note {
        title: String,
        address: String,
        /// 是通过哪条指令来到这一页的（直接打开时为 `null`）
        via: Option<Via>,
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
        /// 版本号：远小于 2^53，前端按普通数字处理即可（默认会导成 bigint，用不了）
        rev: u64,
        id: String,
        short_id: String,
        address: String,
    },
    /// 回退的**二次确认页**（`rollback-版本`）
    RollbackConfirm {
        title: String,
        /// 版本号：远小于 2^53，前端按普通数字处理即可（默认会导成 bigint，用不了）
        rev: u64,
        id: String,
        short_id: String,
        address: String,
    },
    /// 特殊页面：**虚拟命名空间** `special:`。它不对应任何笔记文件，
    /// 由前端按 `page` 渲染（例如 `special:newtab`）。
    Special {
        page: String,
        address: String,
        /// 是通过哪条指令来到这一页的（直接打开时为 `null`）。
        /// 虚拟命名空间下的页面同样要能看到来源 —— 它就是普通页面的一种。
        via: Option<Via>,
    },
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
    /// 附件目录（界面拼取文件地址时要它）
    pub files_dir: String,
    /// 数据库模型版本（`大.中.小`；语义见 `storage::version`）
    pub model_version: String,
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

/// 一个附件（上传的图片、文档……）。
///
/// `name` 是**原始文件名**，笔记里就用它引用（`::image src=图片.png`）——
/// 磁盘上的名字则是生成的标识，见 `storage::files` 的说明。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub id: String,
    pub name: String,
    pub size: u64,
    pub sha256: String,
    pub uploaded: String,
    pub mime: String,
    /// 取文件的地址（程序自己的 `refind:` 方案）
    pub url: String,
}

/// 最近更改里的一条。
///
/// 与某一篇笔记的版本历史是同一份数据，只是**汇总到了一起**、按时间倒序 ——
/// "刚才做了什么"是全仓库的问题，不是某一页的问题。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeEntry {
    pub title: String,
    /// `commit` / `draft` / `delete`
    pub kind: String,
    pub rev: u64,
    pub at: String,
    pub bytes: u64,
    /// 相对上一版的字节增减，一眼看出改了多少
    pub delta: i64,
    /// 展示用的提交短 id
    pub short_id: String,
}
