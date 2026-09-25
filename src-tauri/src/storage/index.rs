//! 索引缓存。
//!
//! 它**只是缓存**：删掉或损坏都能由 `notes/*.log` 重建，避免出现第二个真相来源。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ---------------------------------------------------------------- 索引

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexEntry {
    pub id: String,
    pub ns: i32,
    pub title: String,
    pub rev: u64,
    pub at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexFile {
    pub format: u32,
    pub built_at: String,
    pub notes: HashMap<String, IndexEntry>,
}
