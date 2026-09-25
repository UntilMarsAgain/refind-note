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
}

impl std::fmt::Display for VaultError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "读写失败：{e}"),
            Self::Title(e) => write!(f, "{e}"),
            Self::Json(e) => write!(f, "数据格式错误：{e}"),
            Self::NotFound(t) => write!(f, "找不到笔记《{t}》"),
            Self::NotText(t) => write!(f, "《{t}》不是文本笔记，暂不能在编辑器里打开"),
            Self::Conflict { expected, found } => write!(
                f,
                "提交冲突：草稿基于版本 {expected}，但链上当前是 {found}"
            ),
            Self::Deleted(t) => write!(f, "《{t}》已被删除"),
            Self::RevisionNotFound { title, rev } => write!(f, "《{title}》没有版本 {rev}"),
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
