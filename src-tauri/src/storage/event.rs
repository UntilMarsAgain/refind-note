//! 事件与折叠：日志里的一行，以及由整条日志重放出的当前状态。

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use super::DEFAULT_MIME;
use crate::title::{NamespaceTable, ParsedTitle};

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
        blob: String,
        bytes: u64,
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
    /// 自动保存的草稿：也占一个版本号，挂在 `on` 所指的提交上
    Auto {
        at: String,
        rev: u64,
        blob: String,
        bytes: u64,
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

impl NoteState {
    pub fn key(&self) -> String {
        format!("{}:{}", self.ns, self.title)
    }

    pub fn display(&self, table: &NamespaceTable) -> String {
        ParsedTitle {
            ns: self.ns,
            title: self.title.clone(),
        }
        .display(table)
    }
}

/// 把日志折叠成当前状态。**重放**就是走一遍这个函数。
pub fn fold(events: &[Event]) -> NoteState {
    let mut state = NoteState {
        mime: DEFAULT_MIME.to_string(),
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
                mime,
                supersedes,
                ns,
                title,
                ..
            } => {
                state.rev = *rev;
                state.blob = Some(blob.clone());
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
            Event::Auto { at, blob, on, .. } => {
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

pub fn next_rev(events: &[Event]) -> u64 {
    events.iter().map(Event::rev).max().unwrap_or(0) + 1
}
