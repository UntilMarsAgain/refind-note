//   Refind Note is a note-taking software.
//   Copyright (C) 2026 Until Mars Again
//
//   This program is free software: you can redistribute it and/or modify
//   it under the terms of the GNU Affero General Public License as published by
//   the Free Software Foundation, either version 3 of the License, or
//   (at your option) any later version.
//
//   This program is distributed in the hope that it will be useful,
//   but WITHOUT ANY WARRANTY; without even the implied warranty of
//   MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
//   GNU Affero General Public License for more details.
//
//   You should have received a copy of the GNU Affero General Public License
//   along with this program.  If not, see <http://www.gnu.org/licenses/>.
//! 笔记：名字表、事件日志、草稿槽位。
//!
//! 三件事分得很清：
//!
//! - **名字表**（`titles.json`）：id ↔ 标题。磁盘上的文件名是生成的 ASCII 标识，
//!   原始标题只活在这张表里 —— 于是中文、空格、重名、跨系统的编码问题都不会变成
//!   文件名的一部分。改名也就只是改表里一行。
//! - **事件日志**（`objects/0/<id>.log`）：一行一条，只追加、不重写。当前状态由整条
//!   日志重放（`fold`）出来，所以"改了什么"永远有据可查。
//! - **草稿槽位**（`drafts/<id>`）：**一个文档一个槽位**，可覆盖。
//!   草稿是还没定稿的工作状态，不是历史的一版 —— 把它塞进日志或 blob 仓，
//!   等于拿不可变存储扛高频写。它不占版本、不进仓；封装照**这篇笔记当前的保护**，
//!   但**不签名**（签名留给提交）。
//!
//! 保护是**这篇笔记的属性**（照最新一版落盘时的封装），不是每次写入的参数：不给策略的
//! 提交与草稿都从它出发，不会因为某次没写参数而偷偷降级；要改就显式给一次新策略
//! （见 `commit_with`）。回滚有两种做法：照当前保护重写（`rollback`），
//! 或直接复制那一版的封装（`rollback_copy`）。
//!
//! ## 这一层怎么分的
//!
//! 同一个 `Database` 上的方法按**职责**摊在这几个文件里，各自是一个 `impl` 块：
//!
//! | 文件 | 管什么 |
//! |---|---|
//! | 本文件 | 类型（事件、状态、摘要）、日志重放 `fold`、以及那几个共用的小工具 |
//! | [`index`] | 名字表与定位：标题 ↔ id、日志落在哪、这篇在不在 |
//! | [`commit`] | 建与提交：`create`、`commit*`、改标题，以及按字节读写 |
//! | [`read`] | 读与回退：`read*`、`rollback*`，以及渲染正文与模板嵌入 |
//! | [`history`] | 版本清单、删除、全部页面列表 |
//! | [`drafts`] | 草稿槽位：存取与丢弃 |
//!
//! 方法都挂在 `Database` 上，调用处与拆分前一样写 `database.read(...)`，
//! 不必知道它究竟在哪个文件。

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::storage::codec::{Meta, Protection};
use crate::storage::store::hash_hex;
use crate::vault::database::now;

mod commit;
mod drafts;
mod history;
mod index;
mod read;
#[cfg(test)]
mod tests;

const DEFAULT_MIME: &str = "text/markdown";

/// 笔记正文的自述：内容都是 markdown
fn body_meta() -> Meta {
    Meta {
        mime: DEFAULT_MIME.to_string(),
    }
}

/// 名字表
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Titles {
    /// id → 标题
    pub notes: BTreeMap<String, String>,
    /// id → 标题（已删除，文件在 trash/ 里）
    pub trashed: BTreeMap<String, String>,
}

/// 日志里的一行。任何变更都是追加一条，文件永不重写。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "lowercase")]
pub enum Event {
    /// 笔记建立
    Meta {
        at: String,
        ns: String,
        title: String,
    },
    /// 改名：标题变了，标识与目录都不动
    ///
    /// 标题在日志里（`Meta` 写的那一次），所以改名也得在这里留一条 ——
    /// 光改 `titles.json` 的话，重放日志会把旧名字又读回来。
    Rename { at: String, title: String },
    /// 删除标记：日志挪进回收站**之前**写的一条
    ///
    /// 它不改内容，只是把"什么时候删的"记在日志自己身上 ——
    /// 表里的记录会被清掉、文件的改动时间会被复制打断，这一条不会。
    Del { at: String },
    /// 一次提交
    Rev {
        at: String,
        rev: u64,
        /// 内容落在哪个 blob 上
        blob: String,
        bytes: u64,
        mime: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        summary: Option<String>,
    },
}

/// 由整条日志重放出来的当前状态
#[derive(Debug, Clone, Default)]
pub struct NoteState {
    pub ns: String,
    pub title: String,
    /// 最后一次删除的时间（还原之后仍然留着：删过这件事也是历史）
    pub deleted_at: Option<String>,
    /// 建立时间
    pub created: String,
    /// 最后一次提交时间
    pub modified: String,
    /// 最新一版落在哪个 blob 上（0 版没有正文）
    pub blob: String,
    pub bytes: u64,
    pub rev: u64,
    /// 最后一次提交的说明
    pub summary: Option<String>,
}

/// 一篇笔记
#[derive(Debug, Clone, Serialize)]
pub struct Note {
    /// 规范键，形如 `0:标题`
    pub key: String,
    /// 这一页是指令页时，它的指令信息（界面据此提示"这一页会跳到哪"）
    pub command: Option<crate::vault::command::CommandInfo>,
    pub title: String,
    /// 原样源码
    pub markdown: String,
    /// 阅读用的 HTML。内部链接已经标好红/蓝
    pub html: String,
    pub rev: u64,
    pub created: String,
    pub modified: String,
    pub summary: Option<String>,
    /// 这一版落盘时用了哪些层 —— **看头就知道，不需要口令**
    pub protection: Protection,
}

/// 列表里的一条
#[derive(Debug, Clone, Serialize)]
pub struct NoteSummary {
    pub key: String,
    pub title: String,
    pub rev: u64,
    pub bytes: u64,
    pub created: String,
    pub modified: String,
}

/// 历史上的一版。
///
/// **只有提交**：草稿是每篇一个可覆盖槽位，不进事件链，所以它不是这里的一行。
#[derive(Debug, Clone, Serialize)]
pub struct RevisionSummary {
    /// 版本号，从 1 开始连续
    pub rev: u64,
    pub at: String,
    pub bytes: u64,
    pub summary: Option<String>,
    /// 这一版落盘时用了哪些层 —— 各版可能不同
    pub protection: Protection,
}

/// 交给界面读的结果。
///
/// 读不到**不是**错误：这一页上了锁时要说得出"需要口令"，界面才好把人带到输入口令
/// 那一页去，而不是丢一个看不懂的错误。
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
// 两个分支的大小差得远（`Note` 本来就大：正文 + HTML）。每次读只造一个，
// 为它套一层 Box 只是把这份代价挪到别处，不如就这么放着。
#[allow(clippy::large_enum_variant)]
pub enum Reading {
    /// 读到了
    Ready { note: Note },
    /// 上了锁：需要**这一页**的口令。
    ///
    /// `wrong_passphrase` 表示刚才那把是错的（这次会话里的已被丢掉）——
    /// 界面据此说"再试一次"，而不是让人对着一个没有反应的输入框发呆。
    Locked {
        protection: Protection,
        /// 为什么读不出来，**给人看的原文**。
        ///
        /// 不做归类：gpg 那边可能是"没有私钥"、"代理被取消了"、"解密失败" ——
        /// 该分别说，压成一句"解密失败"等于把人引到错的地方。
        /// 界面据此决定解锁框上写什么、以及要不要给口令输入框（看 `protection`）。
        reason: String,
        wrong_passphrase: bool,
    },
}

/// 一份草稿
#[derive(Debug, Clone, Serialize)]
pub struct Draft {
    pub markdown: String,
    pub modified: String,
    pub protection: Protection,
}

/// 把整条日志折成当前状态
pub fn fold(events: &[Event]) -> NoteState {
    let mut state = NoteState::default();

    for event in events {
        match event {
            Event::Meta { at, ns, title } => {
                state.ns = ns.clone();
                state.title = title.clone();
                state.created = at.clone();
                state.modified = at.clone();
            }
            Event::Rename { title, .. } => {
                state.title = title.clone();
            }
            Event::Del { at } => {
                state.deleted_at = Some(at.clone());
            }
            Event::Rev {
                at,
                rev,
                blob,
                bytes,
                summary,
                ..
            } => {
                state.modified = at.clone();
                state.blob = blob.clone();
                state.bytes = *bytes;
                state.rev = *rev;
                state.summary = summary.clone();
            }
        }
    }

    state
}

/// 这个错是不是"口令不对"。
///
/// 认的是那句话本身，而它的写法只有 `codec` 一处 —— 两边不会各自漂走。
/// 装配一版正文时的失败。
///
/// 分两类的理由见 [`AssembleError::codec`]：上层要分辨"上了锁"（有出路，摆个框让人
/// 解锁）与"这东西坏了"（没出路）。以前这个分辨是在 `Display` 出来的字符串里找字，
/// 而 gpg 的失败压根不在那句话里 —— 于是它被当成"坏了"，人看到一句技术话，
/// 既没有框也没有重试。
#[derive(Debug)]
pub(super) enum AssembleError {
    /// 解层失败（口令不对 / 要口令 / gpg 那边的问题 / 没装 gpg）
    Codec(crate::storage::codec::CodecError),
    /// 不是解层的事（渲染失败、正文不是文本……）
    Other(String),
}

impl AssembleError {
    /// 解层那一层的错误（`Other` 时没有）
    pub(super) fn codec(&self) -> Option<&crate::storage::codec::CodecError> {
        match self {
            Self::Codec(error) => Some(error),
            Self::Other(_) => None,
        }
    }

    /// 是不是"上了锁 / 解不开"，而不是"东西坏了"
    pub(super) fn is_lock(&self) -> bool {
        self.codec().is_some_and(|error| error.is_lock())
    }

    /// 刚才是"口令不对"（可以让人改了再输一次）
    pub(super) fn is_wrong_passphrase(&self) -> bool {
        self.codec()
            .is_some_and(|error| error.is_wrong_passphrase())
    }
}

impl From<crate::storage::codec::CodecError> for AssembleError {
    fn from(error: crate::storage::codec::CodecError) -> Self {
        Self::Codec(error)
    }
}

impl fmt::Display for AssembleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Codec(error) => write!(f, "{error}"),
            Self::Other(reason) => write!(f, "{reason}"),
        }
    }
}

/// 生成一个没被用过的 id：纯 ASCII 十六进制，撞了就再取一个
fn next_id(titles: &Titles) -> String {
    let mut salt = 0u64;
    loop {
        let id: String = hash_hex(format!("{}:{salt}", now()).as_bytes())
            .chars()
            .take(16)
            .collect();
        if !titles.notes.contains_key(&id) && !titles.trashed.contains_key(&id) {
            return id;
        }
        salt += 1;
    }
}

/// 文件的修改时间（RFC3339）。取不到就给空串 —— 它只是给人看的。
fn modified_at(path: &Path) -> String {
    let Ok(modified) = fs::metadata(path).and_then(|meta| meta.modified()) else {
        return String::new();
    };
    let Ok(stamp) = modified.duration_since(std::time::UNIX_EPOCH) else {
        return String::new();
    };
    let Ok(moment) = time::OffsetDateTime::from_unix_timestamp(stamp.as_secs() as i64) else {
        return String::new();
    };
    moment
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}
