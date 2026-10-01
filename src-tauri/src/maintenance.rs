//! 回收站与仓库整理。
//!
//! 两条互补的规矩：
//!
//! - **删除不抹掉**：日志挪进 `trash/<命名空间>/`，历史一条没少，随时能还原；
//! - **空间要人说了算**：blob 只有在**没有任何日志引用**之后才会被回收，
//!   而"没人引用"这件事得等它所在的笔记被永久清除之后才成立。
//!
//! 于是回收分两步：先把回收站里的东西清除（那一步不可撤销），再扫一遍 blob 仓。
//! 扫的时候只认**日志里的引用**（`Rev` 事件带的 blob），不做任何猜测 ——
//! 宁可留着一个没人要的 blob，也不要删掉一个还有人指着的内容。

use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::database::{now, Database};
use crate::notes::{fold, Event};

/// 回收站里的一条
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrashEntry {
    /// 显示标题（还原时按它找）
    pub title: String,
    /// 删除时间（RFC3339）；日志里没有删除标记时是空串
    pub deleted_at: String,
    /// 日志文件自己的体积
    pub bytes: u64,
    /// 删了几天了；算不出来（时间缺失或读不动）就是 `None`
    pub days_old: Option<i64>,
}

/// 一次仓库整理的账单
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GcReport {
    /// 回收掉的内容块个数
    pub removed_blobs: u64,
    /// 释放的字节数
    pub freed_bytes: u64,
    /// 清掉的孤儿草稿槽位个数
    pub removed_drafts: u64,
}

/// 永久清除回收站条目的账单
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PurgeReport {
    pub removed: u64,
    pub freed_bytes: u64,
    /// 清除之后顺带回收的
    pub gc: GcReport,
}

/// 开机自动维护的账单：`None` = 这一轮不用做
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MaintenanceReport {
    pub purged: Option<PurgeReport>,
    pub gc: Option<GcReport>,
}

impl Database {
    // ---------------------------------------------------------------- 回收站

    /// 回收站里的条目，**新的在前**。
    ///
    /// 以 `titles.trashed` 为准，但**目录说了算**：文件已经不在的条目不算数
    /// （回收站里没有它，就不该在列表里出现）。
    pub fn list_trash(&self) -> Result<Vec<TrashEntry>, String> {
        let titles = self.titles()?;
        let mut out = Vec::new();

        for (id, display) in &titles.trashed {
            let path = self.trash_path(id);
            let Ok(meta) = fs::metadata(&path) else {
                continue;
            };

            let deleted_at = self.deleted_at(id)?;
            out.push(TrashEntry {
                title: display.clone(),
                days_old: days_since(&deleted_at),
                deleted_at,
                bytes: meta.len(),
            });
        }

        // 新的在前；时间读不出来的排在最后（它们也永远不会被自动清掉）
        out.sort_by(|a, b| b.deleted_at.cmp(&a.deleted_at));
        Ok(out)
    }

    /// 这一篇是什么时候被删的：日志里**最后一条**删除标记
    fn deleted_at(&self, id: &str) -> Result<String, String> {
        let path = self.trash_path(id);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(_) => return Ok(String::new()),
        };

        let mut at = String::new();
        for line in text.lines() {
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(Event::Del { at: stamp }) = serde_json::from_str::<Event>(line) {
                at = stamp;
            }
        }
        Ok(at)
    }

    /// 还原：文件挪回去，标题回到 `notes`，并**追加一版**留个痕迹。
    ///
    /// 那一版复用删除前的内容（同一个 blob），所以还原出来的还是原来那篇；
    /// 删除标记留在日志里，"什么时候删过"也还查得到。
    pub fn restore(&self, title: &str) -> Result<crate::notes::Reading, String> {
        let mut titles = self.titles()?;
        let Some((id, display)) = titles
            .trashed
            .iter()
            .find(|(_, name)| name.as_str() == title)
            .map(|(id, name)| (id.clone(), name.clone()))
        else {
            return Err(format!("回收站里没有「{title}」"));
        };

        let from = self.trash_path(&id);
        if !from.is_file() {
            return Err(format!("回收站里没有「{title}」的文件"));
        }

        let events = self.read_trashed_events(&id)?;
        let state = fold(&events);
        if state.rev == 0 || state.blob.is_empty() {
            return Err(format!("「{title}」没有可还原的内容"));
        }

        // 挪回去要的目录先备好：命名空间可能是后来才建的
        let ns = crate::title::parse(&display, &self.namespaces())
            .map(|parsed| parsed.ns)
            .unwrap_or_else(|_| crate::namespace::MAIN_ID.to_string());
        self.ensure_namespace_dir(&ns)?;

        let to = self.note_path(&ns, &id);
        fs::rename(&from, &to).map_err(|error| format!("挪不动 {}：{error}", from.display()))?;

        titles.trashed.remove(&id);
        titles.notes.insert(id.clone(), display.clone());
        self.save_titles(&titles)?;

        // 还原也记成一次提交：历史里看得见"从回收站还原"
        self.append_restored(&id, &state.blob, state.bytes)?;
        self.read(&display)
            .map(|note| crate::notes::Reading::Ready { note })
    }

    /// 永久清除一条：日志删掉、表里的记录删掉。
    ///
    /// **不管 blob** —— 内容块要等整份日志都不在了才谈得上"没人引用"，
    /// 那是 [`Self::gc`] 的事（页面上也这么写着，一步一步来）。
    pub fn purge_trash_entry(&self, title: &str) -> Result<(), String> {
        let mut titles = self.titles()?;
        let Some(id) = titles
            .trashed
            .iter()
            .find(|(_, name)| name.as_str() == title)
            .map(|(id, _)| id.clone())
        else {
            return Err(format!("回收站里没有「{title}」"));
        };

        let path = self.trash_path(&id);
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("删不掉 {}：{error}", path.display())),
        }

        titles.trashed.remove(&id);
        self.save_titles(&titles)
    }

    /// 清掉回收站里超过 `days` 天的条目（`0` = 全部）。
    ///
    /// 时间读不出来的条目**一律不碰**：宁可不删，也不要误删。
    pub fn purge_trash(&self, days: i64) -> Result<PurgeReport, String> {
        let mut report = PurgeReport::default();
        let mut freed = 0u64;

        for entry in self.list_trash()? {
            let old_enough = if days <= 0 {
                true
            } else {
                matches!(entry.days_old, Some(age) if age >= days)
            };
            if !old_enough {
                continue;
            }

            freed += entry.bytes;
            self.purge_trash_entry(&entry.title)?;
            report.removed += 1;
        }

        report.freed_bytes = freed;
        // 日志没了，它们引用的内容块这才成为孤儿 —— 顺手回收
        if report.removed > 0 {
            report.gc = self.gc(true, false)?;
        }
        Ok(report)
    }

    // ---------------------------------------------------------------- 仓库整理

    /// 整理一遍。
    ///
    /// 两件事各自独立：
    /// - `orphan_blobs`：删掉**没有任何日志引用**的内容块；
    /// - `orphan_drafts`：删掉**没有主的草稿槽位**（笔记已经被永久清除了）。
    ///
    /// 顺序不能反：先清草稿槽位不影响 blob 的引用（草稿槽位是独立文件，不进 blob 仓）。
    pub fn gc(&self, orphan_blobs: bool, orphan_drafts: bool) -> Result<GcReport, String> {
        let mut report = GcReport::default();

        if orphan_drafts {
            let live: HashSet<String> = {
                let titles = self.titles()?;
                titles
                    .notes
                    .keys()
                    .chain(titles.trashed.keys())
                    .cloned()
                    .collect()
            };

            let dir = self.drafts_dir();
            if let Ok(entries) = fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let id = entry.file_name().to_string_lossy().to_string();
                    if live.contains(&id) {
                        continue;
                    }
                    if fs::remove_file(entry.path()).is_ok() {
                        report.removed_drafts += 1;
                    }
                }
            }
        }

        if orphan_blobs {
            let referenced = self.referenced_blobs()?;
            let store = self.blobs();

            for (address, size) in store.list()? {
                // 写了一半留下的渣：它本来就不该在
                let is_residue = address.ends_with(".tmp");
                if !is_residue && referenced.contains(&address) {
                    continue;
                }
                if store.remove(&address).is_ok() {
                    report.removed_blobs += 1;
                    report.freed_bytes += size;
                }
            }
        }

        Ok(report)
    }

    /// 全部日志引用到的 blob 地址（`objects/` 与 `trash/` 都算）。
    ///
    /// 用集合而不是计数器：内容寻址之下，一个 blob 被引用的次数是多少都不影响结论 ——
    /// 只要还有人指着它，它就得在。
    fn referenced_blobs(&self) -> Result<HashSet<String>, String> {
        let mut out = HashSet::new();
        for path in self.log_files()? {
            let Ok(text) = fs::read_to_string(&path) else {
                // 读不动一份日志就保守一点：当它引用了什么都可能 ——
                // 这里没法"保守"，索性跳过整轮回收会让别的也做不成；
                // 折中是**不删任何东西**：直接报错，让人来看一眼
                return Err(format!("读不出 {}，先别回收", path.display()));
            };

            for line in text.lines() {
                if line.trim().is_empty() {
                    continue;
                }
                let Ok(event) = serde_json::from_str::<Event>(line) else {
                    continue;
                };
                if let Event::Rev { blob, .. } = event {
                    out.insert(blob);
                }
            }
        }
        Ok(out)
    }

    /// 库里全部日志文件的路径（`objects/**/*.log` 与 `trash/**/*.log`）
    fn log_files(&self) -> Result<Vec<PathBuf>, String> {
        let mut out = Vec::new();
        for base in [self.objects_dir(), self.trash_dir()] {
            let Ok(namespaces) = fs::read_dir(&base) else {
                continue;
            };
            for namespace in namespaces.flatten() {
                let Ok(entries) = fs::read_dir(namespace.path()) else {
                    continue;
                };
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().is_some_and(|ext| ext == "log") {
                        out.push(path);
                    }
                }
            }
        }
        Ok(out)
    }

    /// 读回收站里的日志（`read_events` 走的是 `objects/`，这里要另一条路）
    fn read_trashed_events(&self, id: &str) -> Result<Vec<Event>, String> {
        let path = self.trash_path(id);
        let text = fs::read_to_string(&path)
            .map_err(|error| format!("读不出 {}：{error}", path.display()))?;

        let mut events = Vec::new();
        for line in text.lines() {
            if line.trim().is_empty() {
                continue;
            }
            events.push(
                serde_json::from_str(line)
                    .map_err(|error| format!("{} 里有一行读不出来：{error}", path.display()))?,
            );
        }
        Ok(events)
    }

    // ---------------------------------------------------------------- 自动维护

    /// 该做维护了没有：距上次执行够不够间隔（首次执行时一律算够）。
    ///
    /// 只看天，不看钟点 —— "每天一次"是这个意思就够了。
    pub fn maintenance_due(&self) -> bool {
        let config = self.config();
        due_at(&config.last_gc, config.gc_interval_days)
            || due_at(&config.last_trash_purge, config.trash_keep_days)
    }

    /// 开机跑一次维护：**先清过期回收站，再整理**。
    ///
    /// 两件事各有各的间隔，各自到期才做；都没到期就返回 `None`（不落任何时间戳，
    /// 免得"什么都没做"也把下一次推后）。
    pub fn run_maintenance(&self) -> Result<Option<MaintenanceReport>, String> {
        let mut config = self.config();
        let mut report = MaintenanceReport::default();

        if due_at(&config.last_trash_purge, config.trash_keep_days) {
            let purged = self.purge_trash(config.trash_keep_days as i64)?;
            config.last_trash_purge = now();
            report.purged = Some(purged);
        }

        if due_at(&config.last_gc, config.gc_interval_days) {
            // 自动那一轮**只回收孤儿内容块**：草稿槽位是人写了一半的东西，
            // 该不该清得由人点头（见整理页的选项）
            let gc = self.gc(true, false)?;
            config.last_gc = now();
            report.gc = Some(gc);
        }

        if report.purged.is_none() && report.gc.is_none() {
            return Ok(None);
        }
        self.save_config(&config)?;
        Ok(Some(report))
    }
}

/// 距上次执行够不够间隔。空串（从没做过）一律算够。
fn due_at(last: &str, interval_days: u64) -> bool {
    if last.is_empty() {
        return true;
    }
    match days_since(last) {
        Some(days) => days >= interval_days.max(1) as i64,
        // 时间读不出来：当作没做过，做一遍总比永远不做强
        None => true,
    }
}

/// 一个 RFC3339 时间距今多少天；读不出来就是 `None`
fn days_since(at: &str) -> Option<i64> {
    let then = OffsetDateTime::parse(at, &Rfc3339).ok()?;
    Some((OffsetDateTime::now_utc() - then).whole_days().max(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> Database {
        let dir =
            std::env::temp_dir().join(format!("refind-note-gc-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let workspace = crate::workspace::Workspace::open(dir).unwrap();
        Database::open(&workspace).unwrap()
    }

    fn cleanup(database: &Database) {
        if let Some(root) = database.root().parent() {
            let _ = fs::remove_dir_all(root);
        }
    }

    /// 写一篇有内容的笔记，返回它的标识
    fn note(database: &Database, title: &str, body: &str) -> String {
        database.create(title).unwrap();
        database.commit(title, body, None).unwrap();
        database.id_of(title).unwrap()
    }

    #[test]
    fn deleting_moves_the_log_to_the_trash_with_a_marker() {
        let database = scratch("delete");
        let id = note(&database, "甲", "旧内容");

        database.delete("甲").unwrap();

        let trashed = database.trash_path(&id);
        assert!(trashed.is_file());
        let text = fs::read_to_string(&trashed).unwrap();
        assert!(
            text.contains(r#""t":"del""#),
            "日志里应当留下删除标记：{text}"
        );

        let listed = database.list_trash().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].title, "甲");
        assert!(!listed[0].deleted_at.is_empty(), "删除时间要从标记里读出来");
        assert_eq!(listed[0].days_old, Some(0));

        cleanup(&database);
    }

    #[test]
    fn restoring_brings_the_note_back_and_says_so_in_the_history() {
        let database = scratch("restore");
        note(&database, "甲", "正文");
        database.delete("甲").unwrap();

        let reading = database.restore("甲").unwrap();
        let crate::notes::Reading::Ready { note } = reading else {
            panic!("还原出来的应当读得到");
        };
        assert_eq!(note.markdown, "正文");
        assert_eq!(note.summary.as_deref(), Some("从回收站还原"));

        assert!(database.exists("甲"));
        assert!(database.log_path(&database.id_of("甲").unwrap()).is_file());
        assert!(database.list_trash().unwrap().is_empty());

        cleanup(&database);
    }

    #[test]
    fn purging_one_entry_does_not_collect_blobs() {
        let database = scratch("purge-one");
        let id = note(&database, "甲", "很长的正文，用来占一点空间");
        let blob = database.state_of(&id).unwrap().blob;

        database.delete("甲").unwrap();
        database.purge_trash_entry("甲").unwrap();

        assert!(database.list_trash().unwrap().is_empty());
        assert!(!database.trash_path(&id).is_file());
        // 内容块还在：清一条不顺手回收，那是整理页的事
        assert!(database.blobs().has(&blob));

        cleanup(&database);
    }

    #[test]
    fn gc_reclaims_the_blobs_of_a_purged_note() {
        let database = scratch("gc-blobs");
        let id = note(&database, "甲", "一段只属于这一篇的正文");
        let blob = database.state_of(&id).unwrap().blob;

        database.delete("甲").unwrap();
        database.purge_trash_entry("甲").unwrap();

        let report = database.gc(true, false).unwrap();
        assert_eq!(report.removed_blobs, 1);
        assert!(report.freed_bytes > 0);
        assert!(!database.blobs().has(&blob));

        cleanup(&database);
    }

    #[test]
    fn gc_keeps_the_blobs_of_notes_still_in_the_trash() {
        let database = scratch("gc-keeps-trash");
        let id = note(&database, "甲", "删了但还在回收站里");
        let blob = database.state_of(&id).unwrap().blob;

        database.delete("甲").unwrap();
        let report = database.gc(true, false).unwrap();

        assert_eq!(report.removed_blobs, 0);
        assert!(
            database.blobs().has(&blob),
            "回收站里的笔记还会被还原，它引用的内容不能动"
        );

        cleanup(&database);
    }

    #[test]
    fn gc_clears_only_drafts_without_an_owner() {
        let database = scratch("gc-drafts");
        note(&database, "甲", "正文");
        note(&database, "乙", "另一篇");
        database.save_draft("乙", "写了一半").unwrap();

        let orphan = database.draft_path("不存在的标识");
        crate::workspace::write_bytes(&orphan, "没有主的槽位".as_bytes()).unwrap();

        let report = database.gc(false, true).unwrap();
        assert_eq!(report.removed_drafts, 1);
        assert!(!orphan.exists(), "没有主的槽位该清掉");
        assert!(
            database.draft_path(&database.id_of("乙").unwrap()).exists(),
            "有主的草稿一个都不能动"
        );

        cleanup(&database);
    }

    #[test]
    fn purge_trash_respects_age_and_cleans_up_after_itself() {
        let database = scratch("purge-age");
        let id = note(&database, "甲", "一段正文");
        let blob = database.state_of(&id).unwrap().blob;
        database.delete("甲").unwrap();

        // 保留 0 天 = 全清；清完顺手回收，那一片内容块这时才成为孤儿
        let report = database.purge_trash(0).unwrap();
        assert_eq!(report.removed, 1);
        assert_eq!(report.gc.removed_blobs, 1, "没有日志引用它之后才谈得上回收");
        assert!(!database.blobs().has(&blob));

        cleanup(&database);
    }

    #[test]
    fn a_fresh_entry_survives_a_purge_by_age() {
        let database = scratch("purge-keeps-young");
        note(&database, "甲", "正文");
        database.delete("甲").unwrap();

        // 今天删的，留 30 天：不动它
        let report = database.purge_trash(30).unwrap();
        assert_eq!(report.removed, 0);
        assert_eq!(database.list_trash().unwrap().len(), 1);

        cleanup(&database);
    }

    #[test]
    fn maintenance_runs_once_per_interval() {
        let database = scratch("maintenance");
        note(&database, "甲", "正文");
        database.delete("甲").unwrap();

        // 从没做过：这一轮该做，而且做完了要记下时间
        let first = database.run_maintenance().unwrap().expect("第一次应当到期");
        assert!(first.gc.is_some());
        assert!(!database.config().last_gc.is_empty());

        // 刚做过：这一轮什么都不用做
        assert!(
            database.run_maintenance().unwrap().is_none(),
            "间隔之内不该再跑"
        );

        cleanup(&database);
    }

    #[test]
    fn the_interval_is_counted_in_days_and_defaults_to_one() {
        let database = scratch("interval");
        assert_eq!(database.config().gc_interval_days, 1, "默认每天一次");

        // 把上次执行时间拨到昨天：又该做了
        let mut config = database.config();
        let yesterday = OffsetDateTime::now_utc() - time::Duration::days(1);
        config.last_gc = yesterday.format(&Rfc3339).unwrap();
        config.last_trash_purge = config.last_gc.clone();
        database.save_config(&config).unwrap();

        assert!(database.maintenance_due());

        cleanup(&database);
    }
}
