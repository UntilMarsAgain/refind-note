//! 仓库的错误类型。

use crate::title::TitleError;
use std::io;

// ---------------------------------------------------------------- 错误

#[derive(Debug)]
pub enum VaultError {
    Io(io::Error),
    Title(TitleError),
    Json(serde_json::Error),
    NotFound(String),
    NotText(String),
    /// 提交时的冲突守卫：草稿基于的版本已经不是链上当前版本
    Conflict { expected: u64, found: u64 },
    Deleted(String),
    /// 要看的版本号在历史里不存在
    RevisionNotFound { title: String, rev: u64 },
    /// 改名时目标标题已经被别的笔记占用
    NameTaken(String),
    /// 数据本身坏了：补丁与基准对不上、增量链断裂等
    Corrupt(String),
    /// 用缩写 ID 找版本时撞上了多个（像 git 那样报歧义，不要猜）
    AmbiguousRevision { prefix: String, matches: usize },
    /// 地址栏那一行本身写错了
    BadAddress(String),
    /// 版本 ID 撞车了：宁可拒绝写入，也不要留下两个「同一个 ID」的版本
    IdCollision(String),
}

impl std::fmt::Display for VaultError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "读写失败：{e}"),
            Self::Title(e) => write!(f, "{e}"),
            Self::Json(e) => write!(f, "数据格式错误：{e}"),
            Self::NotFound(t) => write!(f, "找不到笔记 {t}"),
            Self::NotText(t) => write!(f, "{t} 不是文本笔记，暂不能在编辑器里打开"),
            Self::Conflict { expected, found } => write!(
                f,
                "提交冲突：草稿基于版本 {expected}，但链上当前是 {found}"
            ),
            Self::Deleted(t) => write!(f, "{t} 已被删除"),
            Self::RevisionNotFound { title, rev } => {
                // rev 为 0 表示"给不出具体版本号"（例如按 commit ID 前缀没找到、
                // 或引用的写法无法解析成版本号）。0 不是真实版本，不该打出来 ——
                // 否则任何这类失败都会显示成"没有版本 0"。
                if *rev == 0 {
                    write!(f, "{title} 没有这个版本")
                } else {
                    write!(f, "{title} 没有版本 {rev}")
                }
            }
            Self::NameTaken(t) => write!(f, "{t} 已经存在，换一个名字"),
            Self::Corrupt(why) => write!(f, "数据损坏：{why}"),
            Self::AmbiguousRevision { prefix, matches } => write!(
                f,
                "「{prefix}」这个缩写对上了 {matches} 个版本，请多写几位"
            ),
            Self::BadAddress(why) => write!(f, "{why}"),
            Self::IdCollision(id) => write!(
                f,
                "版本标识与已有版本重复（{id}…）。为安全起见拒绝写入，请先备份仓库。"
            ),
        }
    }
}

impl std::error::Error for VaultError {}

impl From<io::Error> for VaultError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for VaultError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

impl From<TitleError> for VaultError {
    fn from(value: TitleError) -> Self {
        Self::Title(value)
    }
}
