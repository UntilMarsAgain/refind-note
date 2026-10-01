//! 浏览历史：**我看过哪些页面**。
//!
//! 它不属于仓库内容，属于"这台机器上这个人" —— 所以落在 `settings/browsing.json`，
//! 与偏好放在一处（那边还有"记不记"这个开关）。换一份仓库读的是另一份历史，
//! 这也对：就像浏览器历史不属于某个网站。
//!
//! 按**地址**去重：再访问一次只是把它挪到最前面、换个时间，不新增一行。

use std::fs;

use serde::{Deserialize, Serialize};

use crate::vault::database::{now, Database};

const BROWSING_FILE: &str = "browsing.json";

/// 最多记这么多条（再来的把最旧的挤掉）
pub const HISTORY_LIMIT: usize = 300;

/// 看过的一页
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Visit {
    /// 当时地址栏里那一串（含状态与章节）—— 点回去时回到的是**那一刻的页面**
    pub address: String,
    /// 当时的标题（后来改名了也照旧，历史就是历史）
    pub title: String,
    /// 访问时间（RFC3339）
    pub at: String,
}

impl Database {
    fn browsing_path(&self) -> std::path::PathBuf {
        self.settings_dir().join(BROWSING_FILE)
    }

    /// 全部记录，**新的在前**
    pub fn browsing(&self) -> Vec<Visit> {
        crate::storage::workspace::read_json::<Vec<Visit>>(&self.browsing_path())
    }

    fn save_browsing(&self, visits: &[Visit]) -> Result<(), String> {
        crate::storage::workspace::write_json(&self.browsing_path(), &visits.to_vec())
    }

    /// 记一次访问：同一地址只留一条，挪到最前面。返回记完之后的整份清单。
    pub fn record_visit(&self, address: &str, title: &str) -> Result<Vec<Visit>, String> {
        let address = address.trim();
        if address.is_empty() {
            return Ok(self.browsing());
        }

        let mut visits = self.browsing();
        visits.retain(|visit| visit.address != address);
        visits.insert(
            0,
            Visit {
                address: address.to_string(),
                title: title.to_string(),
                at: now(),
            },
        );
        visits.truncate(HISTORY_LIMIT);

        self.save_browsing(&visits)?;
        Ok(visits)
    }

    /// 清空（记不记那个开关不动：关掉只是不再记新的）
    pub fn clear_browsing(&self) -> Result<(), String> {
        let path = self.browsing_path();
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(format!("清不掉 {}：{error}", path.display())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> Database {
        let dir = std::env::temp_dir().join(format!(
            "refind-note-browsing-test-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        let workspace = crate::storage::workspace::Workspace::open(dir).unwrap();
        Database::open(&workspace).unwrap()
    }

    fn cleanup(database: &Database) {
        if let Some(root) = database.root().parent() {
            let _ = fs::remove_dir_all(root);
        }
    }

    #[test]
    fn revisited_pages_move_to_the_front_instead_of_piling_up() {
        let database = scratch("dedupe");
        database.record_visit("甲", "甲").unwrap();
        database.record_visit("乙", "乙").unwrap();
        let visits = database.record_visit("甲", "甲").unwrap();

        assert_eq!(visits.len(), 2, "同一个地址只留一条");
        assert_eq!(visits[0].address, "甲");
        assert_eq!(visits[1].address, "乙");
        assert!(!visits[0].at.is_empty());

        cleanup(&database);
    }

    #[test]
    fn the_list_survives_a_reopen() {
        let dir = std::env::temp_dir().join(format!(
            "refind-note-browsing-test-{}-persist",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);

        {
            let workspace = crate::storage::workspace::Workspace::open(dir.clone()).unwrap();
            let database = Database::open(&workspace).unwrap();
            database.record_visit("甲@view-2", "甲").unwrap();
        }

        let workspace = crate::storage::workspace::Workspace::open(dir.clone()).unwrap();
        let reopened = Database::open(&workspace).unwrap();
        let visits = reopened.browsing();
        assert_eq!(visits.len(), 1);
        assert_eq!(visits[0].address, "甲@view-2", "带状态的地址原样留着");
        assert_eq!(visits[0].title, "甲");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn clearing_leaves_a_clean_slate() {
        let database = scratch("clear");
        database.record_visit("甲", "甲").unwrap();
        database.clear_browsing().unwrap();
        assert!(database.browsing().is_empty());
        // 再清一次也不报错
        database.clear_browsing().unwrap();
        cleanup(&database);
    }

    #[test]
    fn the_list_is_capped() {
        let database = scratch("cap");
        for index in 0..(HISTORY_LIMIT + 5) {
            database
                .record_visit(&format!("第{index}页"), "页")
                .unwrap();
        }
        let visits = database.browsing();
        assert_eq!(visits.len(), HISTORY_LIMIT);
        assert_eq!(visits[0].address, format!("第{}页", HISTORY_LIMIT + 4));
        cleanup(&database);
    }
}
