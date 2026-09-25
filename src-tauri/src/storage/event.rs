//! 事件与折叠：日志里的一行，以及由整条日志重放出的当前状态。

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use super::atomic::hash_bytes;
use super::DEFAULT_MIME;

// ---------------------------------------------------------------- 事件

/// 日志里的一行。任何变更都是追加一条，文件永不重写（除非显式 prune）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "lowercase")]
pub enum Event {
    /// 笔记建立
    Meta {
        at: String,
        ns: i32,
        title: String,
    },
    /// 一次提交
    Rev {
        at: String,
        rev: u64,
        /// 内容落在哪个 blob 上：整份就是正文，增量就是补丁
        blob: String,
        bytes: u64,
        /// `full`（整份）或 `delta`（相对 `base_rev` 的补丁）
        #[serde(default = "default_encoding")]
        encoding: String,
        /// 增量的基准版本号。**一定是提交**——提交不能依赖草稿
        #[serde(default, skip_serializing_if = "Option::is_none")]
        base_rev: Option<u64>,
        /// 这一版完整内容的哈希（增量时也记，便于判断内容是否变过）
        #[serde(default, skip_serializing_if = "String::is_empty")]
        content_hash: String,
        mime: String,
        #[serde(default)]
        parent: Option<u64>,
        /// 本次提交取代掉的草稿版本号（这些草稿节点随时可以删）
        #[serde(default)]
        supersedes: Vec<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ns: Option<i32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        summary: Option<String>,
    },
    /// 自动保存的草稿：也占一个版本号，挂在 `on` 所指的提交上。
    ///
    /// 草稿一律**整份存**：它们随时会被提交取代、被清理，增量省下的那点字节不值得
    /// 让清理逻辑去照顾增量链。（规则 3 允许草稿依赖草稿，这里作为备用能力保留。）
    Auto {
        at: String,
        rev: u64,
        blob: String,
        bytes: u64,
        #[serde(default = "default_encoding")]
        encoding: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        base_rev: Option<u64>,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        content_hash: String,
        mime: String,
        on: u64,
    },
    /// 删除标记（不是抹除）
    Del {
        at: String,
        rev: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        summary: Option<String>,
    },
}

/// 老日志没写 `encoding` 时按整份内容理解
fn default_encoding() -> String {
    "full".to_string()
}

impl Event {
    fn rev(&self) -> u64 {
        match self {
            Self::Meta { .. } => 0,
            Self::Rev { rev, .. } | Self::Auto { rev, .. } | Self::Del { rev, .. } => *rev,
        }
    }
}

/// 折叠日志得到的当前状态
#[derive(Debug, Clone, Default)]
pub struct NoteState {
    pub ns: i32,
    pub title: String,
    /// 当前提交版本；0 表示还没提交过
    pub rev: u64,
    pub blob: Option<String>,
    pub bytes: u64,
    /// 上一版的存法：改名那一版要原样沿用，否则内容引用会错位
    pub encoding: String,
    pub base_rev: Option<u64>,
    pub content_hash: String,
    pub mime: String,
    pub at: String,
    pub deleted: bool,
    /// 最新的未提交草稿（若有）
    pub draft: Option<DraftState>,
    /// 已被提交取代的草稿版本号
    pub superseded: HashSet<u64>,
}

#[derive(Debug, Clone)]
pub struct DraftState {
    pub blob: String,
    pub at: String,
    pub on: u64,
}

/// 把日志折叠成当前状态。**重放**就是走一遍这个函数。
pub fn fold(events: &[Event]) -> NoteState {
    let mut state = NoteState {
        mime: DEFAULT_MIME.to_string(),
        encoding: "full".to_string(),
        ..Default::default()
    };
    let mut pending_draft: Option<DraftState> = None;

    for event in events {
        match event {
            Event::Meta { at, ns, title } => {
                state.ns = *ns;
                state.title = title.clone();
                state.at = at.clone();
            }
            Event::Rev {
                at,
                rev,
                blob,
                bytes,
                encoding,
                base_rev,
                content_hash,
                mime,
                supersedes,
                ns,
                title,
                ..
            } => {
                state.rev = *rev;
                state.blob = Some(blob.clone());
                state.bytes = *bytes;
                state.encoding = encoding.clone();
                state.base_rev = *base_rev;
                state.content_hash = content_hash.clone();
                state.mime = mime.clone();
                state.at = at.clone();
                state.deleted = false;
                if let Some(ns) = ns {
                    state.ns = *ns;
                }
                if let Some(title) = title {
                    state.title = title.clone();
                }
                state.superseded.extend(supersedes.iter().copied());
                // 一次提交就结束了此前那一串草稿
                pending_draft = None;
            }
            Event::Auto {
                at,
                bytes,
                blob,
                encoding,
                content_hash,
                on,
                ..
            } => {
                state.bytes = *bytes;
                state.encoding = encoding.clone();
                state.content_hash = content_hash.clone();
                pending_draft = Some(DraftState {
                    blob: blob.clone(),
                    at: at.clone(),
                    on: *on,
                });
            }
            Event::Del { at, rev, .. } => {
                state.rev = *rev;
                state.deleted = true;
                state.at = at.clone();
            }
        }
    }

    state.draft = pending_draft;
    state
}

/// 一个版本的**稳定 ID**（git 式 commit id）。
///
/// 刻意不把 ID 写进日志：它完全由事件自身的判别信息决定，读的时候算出来即可 ——
/// 这与 git「对象内容决定哈希」是同一个办法，日志里也不会多出一个必须同步维护的字段。
///
/// ⚠️ 代价与 git 相同：这个派生规则一旦改动，旧的 ID 就全变了。所以这里的字段
/// 只许加、不许改含义。
pub fn revision_id(event: &Event) -> String {
    let identity = match event {
        Event::Meta { at, ns, title } => format!("meta:{at}:{ns}:{title}"),
        Event::Rev {
            at,
            rev,
            blob,
            bytes,
            encoding,
            base_rev,
            mime,
            parent,
            ns,
            title,
            summary,
            content_hash,
            ..
        } => format!(
            "rev:{rev}:{at}:{blob}:{bytes}:{encoding}:{base_rev:?}:{content_hash}:{mime}:{parent:?}:{ns:?}:{title:?}:{summary:?}"
        ),
        Event::Auto {
            at,
            rev,
            blob,
            bytes,
            encoding,
            base_rev,
            mime,
            on,
            content_hash,
            ..
        } => format!(
            "auto:{rev}:{at}:{blob}:{bytes}:{encoding}:{base_rev:?}:{content_hash}:{mime}:{on}"
        ),
        Event::Del { at, rev, summary } => format!("del:{rev}:{at}:{summary:?}"),
    };

    hash_bytes(identity.as_bytes())
}

/// 事件对应的版本号；只有带正文的事件（提交 / 草稿）才算可跳转的版本
pub fn revision_of(event: &Event) -> Option<u64> {
    match event {
        Event::Rev { rev, .. } | Event::Auto { rev, .. } => Some(*rev),
        _ => None,
    }
}

/// 展示用的短 ID（git 默认 7 位，这里给 8 位）
pub fn short_revision_id(id: &str) -> String {
    id.chars().take(8).collect()
}

/// 挂在某个提交上的全部草稿版本号（一次提交/改名会取代它们）
pub fn drafts_of(events: &[Event], on_rev: u64) -> Vec<u64> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::Auto { rev, on, .. } if *on == on_rev => Some(*rev),
            _ => None,
        })
        .collect()
}

pub fn next_rev(events: &[Event]) -> u64 {
    events.iter().map(Event::rev).max().unwrap_or(0) + 1
}
