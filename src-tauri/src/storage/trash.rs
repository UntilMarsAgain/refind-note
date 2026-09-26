//! 回收站：清空、清理、还原，以及按间隔自动维护。
//!
//! 从 `mod.rs` 搬出来，理由同上：它只依赖写入与名字表，不依赖地址解析。

use super::*;

impl Vault {
    /// 从回收站还原一篇笔记。
    ///
    /// 做法：文件搬回 `notes/`、名字表挪回去，然后**追加一次提交**（内容不变）——
    /// `fold` 遇到 `Rev` 会把 `deleted` 清掉，于是它重新算"活着"。
    ///
    /// 用追加而不是抹掉那条删除标记：**一条历史都不丢**（"什么时候删过"这件事仍留在链上），
    /// 这也是删除确认页对用户的承诺。
    pub fn restore_note(&self, title: &str) -> Result<Note, VaultError> {
        // 命名空间可能已经被删除：那样就还原不回来了，提示要说清是哪件事
        // （标题里不会出现 `:`，所以有冒号就一定是命名空间前缀）
        if let Some((prefix, _)) = title.split_once(':') {
            if self.table.lookup(prefix).is_none() {
                return Err(VaultError::BadAddress(format!(
                    "命名空间「{}」不存在，无法还原《{title}》",
                    prefix.trim()
                )));
            }
        }

        let (parsed, path) = self.locate(title)?;
        let display = parsed.display(&self.table);
        let id = Self::id_from_path(&path);

        let trashed = self.trashed_path(&id);
        if !trashed.is_file() {
            return Err(VaultError::NotFound(display));
        }

        let events = self.read_events_at(&trashed)?;
        let state = fold(&events);
        if !state.deleted {
            // 文件在回收站里、却没有删除记录：状态不一致，先别动它
            return Err(VaultError::Corrupt(format!(
                "《{display}》在回收站里，但没有删除记录，无法还原"
            )));
        }

        // 先把文件搬回去，再追加"还原"这一版
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::rename(&trashed, &path)?;

        let mut titles = self.titles()?;
        titles.trashed.remove(&id);
        titles.notes.insert(id.clone(), display.clone());
        self.save_titles(&titles)?;

        // 与"改名"同一套做法：复用原有的 blob，只追加一条指向它的提交
        let rev = next_rev(&events);
        self.append(
            &id,
            &Event::Rev {
                at: now_iso(),
                rev,
                blob: state.blob.clone().unwrap_or_default(),
                bytes: state.bytes,
                encoding: state.encoding.clone(),
                base_rev: state.base_rev,
                content_hash: state.content_hash.clone(),
                mime: state.mime.clone(),
                parent: (state.rev > 0).then_some(state.rev),
                supersedes: drafts_of(&events, state.rev),
                ns: None,
                title: None,
                summary: Some("从回收站还原".to_string()),
            },
        )?;

        self.load(&display)
    }

    /// 立即清除回收站里的**一条**（不等保留期）。
    ///
    /// 只删日志与名字表项，**不顺手回收内容块**：那件事交给「数据库回收」那一页，
    /// 由用户决定何时回收 —— 回收站页面正好链过去。
    pub fn purge_trash_entry(&self, title: &str) -> Result<(), VaultError> {
        // 按**名字表里的显示标题**找，而不是先解析标题：命名空间可能已经被删除，
        // 那时解析会失败，可这条记录还得能被清掉 —— 否则它就永久卡在回收站里了。
        let mut titles = self.titles()?;
        let Some(id) = titles
            .trashed
            .iter()
            .find(|(_, stored)| *stored == title)
            .map(|(id, _)| id.clone())
        else {
            return Err(VaultError::NotFound(title.to_string()));
        };

        let trashed = self.trashed_path(&id);
        if !trashed.is_file() {
            return Err(VaultError::NotFound(title.to_string()));
        }
        fs::remove_file(&trashed)?;
        titles.trashed.remove(&id);
        self.save_titles(&titles)?;
        Ok(())
    }

    /// 该跑了吗：用**上次执行的时间**与现在比。
    ///
    /// 从没跑过（空字符串或解析失败）一律算"该跑了"；间隔下限 1 天，
    /// 免得 0 变成"每次启动都清一次"。
    fn maintenance_due(&self, last: &str, interval_days: u64) -> bool {
        let interval = interval_days.max(1);
        match parse_iso(last) {
            None => true,
            Some(at) => {
                let elapsed = (time::OffsetDateTime::now_utc() - at).whole_days();
                elapsed >= interval as i64
            }
        }
    }

    /// 有没有到点该做的维护。
    ///
    /// 单独一个判断，是因为**建一个什么都不做的任务本身就是误导**：任务栏弹出
    /// 「数据库维护」，用户会以为它在干活，反而看不出真实状态 —— 这一次就是这么被发现的。
    pub fn maintenance_pending(&self) -> bool {
        self.maintenance_due(
            &self.config.last_trash_purge.clone(),
            self.config.trash_keep_days,
        ) || self.maintenance_due(&self.config.last_gc.clone(), self.config.gc_interval_days)
    }

    /// 自动维护：到点了就清回收站、回收内容块，并记下这次的时间。
    ///
    /// 判定只看**上次执行时间**（不是"每次启动都跑"）：间隔之内什么都不做。
    pub fn run_maintenance(&mut self) -> Result<MaintenanceReport, VaultError> {
        let mut report = MaintenanceReport::default();
        let mut touched = false;

        if self.maintenance_due(&self.config.last_trash_purge.clone(), self.config.trash_keep_days) {
            let keep = self.config.trash_keep_days as i64;
            report.purged = Some(self.purge_trash(keep)?);
            self.config.last_trash_purge = now_iso();
            touched = true;
        }

        if self.maintenance_due(&self.config.last_gc.clone(), self.config.gc_interval_days) {
            report.gc = Some(self.gc(true, false)?);
            self.config.last_gc = now_iso();
            touched = true;
        }

        if touched {
            self.save_config()?;
        }
        Ok(report)
    }

    /// 回收站清单：删过的笔记，连同"删了多久"。
    ///
    /// 清单的**存在性**来自 `trash/` 目录，名字与删除时间来自名字表和删除标记 ——
    /// 与别处一致：目录说有没有，表说叫什么。
    pub fn list_trash(&self) -> Result<Vec<TrashEntry>, VaultError> {
        let mut out = Vec::new();

        for (id, title) in self.titles()?.trashed {
            let path = self.trashed_path(&id);
            if !path.is_file() {
                // 文件没了却还留在表里：忽略它（以目录为准），不要因此让整页打不开
                continue;
            }
            let bytes = fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
            let events = self.read_events_at(&path)?;
            let at = deletion_time(&events);
            out.push(TrashEntry {
                title,
                deleted_at: at
                    .map(|value| {
                        value
                            .format(&time::format_description::well_known::Rfc3339)
                            .unwrap_or_default()
                    })
                    .unwrap_or_default(),
                bytes,
                days_old: at.map(days_since),
            });
        }

        // 新的在前：这一页的用途是"决定要不要清"，最近删的最需要看见
        out.sort_by(|a, b| b.deleted_at.cmp(&a.deleted_at));
        Ok(out)
    }

    /// 清理回收站：删掉超过 `older_than_days` 天的条目。
    ///
    /// 传 0 表示"全部清理"（任何条目都满足"已删 0 天以上"）。
    /// 清完顺手做一次内容块回收 —— 在此之前，那些块还被回收站的日志引用着，
    /// 只有日志没了它们才真正无人引用。
    pub fn purge_trash(&self, older_than_days: i64) -> Result<PurgeReport, VaultError> {
        let mut report = PurgeReport {
            removed: 0,
            freed_bytes: 0,
            blobs: GcReport::default(),
        };

        let mut titles = self.titles()?;
        let mut purged: Vec<String> = Vec::new();

        for (id, _) in &titles.trashed {
            let path = self.trashed_path(id);
            if !path.is_file() {
                continue;
            }
            let events = self.read_events_at(&path)?;
            let Some(at) = deletion_time(&events) else {
                // 读不出删除时间就不动它：**宁可不删，也不要误删**
                continue;
            };
            if days_since(at) < older_than_days {
                continue;
            }

            report.freed_bytes += fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
            fs::remove_file(&path)?;
            report.removed += 1;
            purged.push(id.clone());
        }

        for id in purged {
            titles.trashed.remove(&id);
        }
        if report.removed > 0 {
            self.save_titles(&titles)?;
            // 日志没了，它们引用的内容块这时才成为孤块
            report.blobs = self.gc(true, false)?;
        }

        Ok(report)
    }
}
