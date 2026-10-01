//! 最近编辑：**跨全部笔记**的版本流水。
//!
//! 数据不另存一份 —— 每次现扫日志（每篇的日志本来就是按时间追加的）。
//! 仓库小的时候这样最省事，也不会出现"索引与日志对不上"。

use serde::Serialize;

use crate::vault::database::Database;
use crate::vault::notes::Event;

/// 流水里的一条
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ChangeEntry {
    /// 显示标题
    pub title: String,
    /// `commit`（提交）/ `delete`（删除）/ `draft`（还没提交的草稿）
    pub kind: String,
    /// 版本号；删除与草稿是 0
    pub rev: u64,
    pub at: String,
    pub bytes: u64,
    /// 与上一版相比的字节增减
    pub delta: i64,
}

/// 默认最多列这么多条
pub const DEFAULT_LIMIT: usize = 100;

impl Database {
    /// 最近的改动，**新的在前**。
    ///
    /// 草稿是**每篇一个槽位**（不是事件），勾上才把它作为一条"还没提交"列进来 ——
    /// 它随时会被覆盖，所以默认不出现。
    pub fn recent_changes(
        &self,
        limit: usize,
        include_drafts: bool,
    ) -> Result<Vec<ChangeEntry>, String> {
        let titles = self.titles()?;
        let mut out: Vec<ChangeEntry> = Vec::new();

        for (id, display) in &titles.notes {
            let events = self.read_events(id)?;
            let mut previous: Option<u64> = None;

            for event in &events {
                match event {
                    Event::Rev { at, rev, bytes, .. } => {
                        let delta = match previous {
                            Some(before) => *bytes as i64 - before as i64,
                            None => *bytes as i64,
                        };
                        previous = Some(*bytes);
                        out.push(ChangeEntry {
                            title: display.clone(),
                            kind: "commit".to_string(),
                            rev: *rev,
                            at: at.clone(),
                            bytes: *bytes,
                            delta,
                        });
                    }
                    Event::Del { at } => out.push(ChangeEntry {
                        title: display.clone(),
                        kind: "delete".to_string(),
                        rev: 0,
                        at: at.clone(),
                        bytes: 0,
                        delta: 0,
                    }),
                    // `Meta` 只说"这篇建立过"，不是一次改动
                    Event::Meta { .. } => {}
                }
            }

            if include_drafts {
                if let Some((at, bytes)) = self.draft_summary(id)? {
                    out.push(ChangeEntry {
                        title: display.clone(),
                        kind: "draft".to_string(),
                        rev: 0,
                        at,
                        bytes,
                        delta: 0,
                    });
                }
            }
        }

        // 时间戳是同一个格式的 RFC3339，按字符串倒序就是按时间倒序
        out.sort_by(|a, b| b.at.cmp(&a.at));
        out.truncate(limit);
        Ok(out)
    }

    /// 草稿槽位的（落盘时间, 字节数）；没有草稿就是 `None`
    fn draft_summary(&self, id: &str) -> Result<Option<(String, u64)>, String> {
        let path = self.draft_path(id);
        let Ok(meta) = std::fs::metadata(&path) else {
            return Ok(None);
        };
        let Ok(modified) = meta.modified() else {
            return Ok(None);
        };
        let at = time::OffsetDateTime::from(modified)
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default();
        Ok(Some((at, meta.len())))
    }
}
