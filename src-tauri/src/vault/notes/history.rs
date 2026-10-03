//! 版本清单、删除与全部页面列表。
//!
//! 删除**不抹掉**日志 —— 它只是挪进 `trash/`，历史一条不少，随时能还原。

use std::fs;

use super::{Event, NoteSummary, RevisionSummary,
            DEFAULT_MIME};
use crate::vault::database::{now, Database};

impl Database {
    /// 这一篇的全部提交，**新的在前**。
    ///
    /// 每一版都带上它自己的存储状态，所以版本列表里就能看出哪几版是加密的。
    pub fn revisions_of(&self, title: &str) -> Result<Vec<RevisionSummary>, String> {
        let id = self
            .id_of(title)
            .ok_or_else(|| format!("没有这篇笔记：{title}"))?;

        let mut out = Vec::new();
        for event in self.read_events(&id)? {
            let Event::Rev {
                at,
                rev,
                blob,
                bytes,
                summary,
                ..
            } = event
            else {
                // `Meta` 只说"这篇建立过"，不是一版
                continue;
            };

            out.push(RevisionSummary {
                rev,
                at,
                bytes,
                summary,
                protection: self.blobs().protection(&blob)?,
            });
        }

        out.reverse();
        Ok(out)
    }

    /// 删除一篇笔记。
    ///
    /// **不是抹掉**：日志挪进 `trash/`，名字从 `notes` 挪到 `trashed` —— 所以还捞得
    /// 回来；blob 一个都不动，回收交给 GC。
    pub fn delete(&self, title: &str) -> Result<(), String> {
        let id = self.locate(title)?;
        let display = self.display_of(title)?;

        // 先留一条删除标记：挪过去之后，日志里还答得出"什么时候删的"
        if self.log_path(&id).is_file() {
            self.append(&id, &Event::Del { at: now() })?;
        }

        let from = self.log_path(&id);
        if from.is_file() {
            let to = self.trash_path(&id);
            if let Some(parent) = to.parent() {
                fs::create_dir_all(parent)
                    .map_err(|error| format!("建不出目录 {}：{error}", parent.display()))?;
            }
            fs::rename(&from, &to)
                .map_err(|error| format!("挪不动 {}：{error}", from.display()))?;
        }

        // 草稿槽位**留着**：它按标识存，而标识不会重用，所以留着既不会串到别人身上，
        // 还原回来时也还接着写得上。没有主的槽位由仓库整理那一轮清（那是人点过头的）。

        let mut titles = self.titles()?;
        titles.notes.remove(&id);
        titles.trashed.insert(id, display);
        self.save_titles(&titles)
    }

    /// 从回收站还原时补的那一版：内容复用原来的 blob，摘要写明来路。
    pub(crate) fn append_restored(&self, id: &str, blob: &str, bytes: u64) -> Result<(), String> {
        let rev = self.state_of(id)?.rev + 1;
        self.append(
            id,
            &Event::Rev {
                at: now(),
                rev,
                blob: blob.to_string(),
                bytes,
                mime: DEFAULT_MIME.to_string(),
                summary: Some("从回收站还原".to_string()),
            },
        )
    }

    /// 列出全部笔记。
    ///
    /// 创建与修改时间**从日志推出来**，不另外存一份 —— 那样才有两个真相来源。
    pub fn list(&self) -> Result<Vec<NoteSummary>, String> {
        let titles = self.titles()?;
        let mut out = Vec::new();

        for (id, display) in &titles.notes {
            let state = self.state_of(id)?;
            out.push(NoteSummary {
                key: format!("{}:{}", state.ns, state.title),
                title: display.clone(),
                rev: state.rev,
                bytes: state.bytes,
                created: state.created,
                modified: state.modified,
            });
        }

        // 最近改过的排前面
        out.sort_by(|a, b| b.modified.cmp(&a.modified));
        Ok(out)
    }
}
