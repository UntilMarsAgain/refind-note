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

//! 这次会话里的那点易失状态。
//!
//! 现在只有一样：口令。
//!
//! 口令**按版本存** —— 同一篇笔记，不同版本可能用了不同的密码，所以解开了第 3 版
//! 不等于第 5 版能读。键是「笔记标识 + 版本号」，两个都是生成的稳定标识，
//! 标题改了、命名空间改名了都不影响。
//!
//! 它**从不落盘**：只活在内存里，程序一关就没了。这也是为什么它不在
//! `preferences.json` 里 —— 那份文件是要跟着仓库走的。

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

type Key = (String, u64);

static PASSWORDS: LazyLock<Mutex<HashMap<Key, String>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// 这一趟里已经读出来的**页正文**：标题 → （版本号, 正文）。
///
/// 为什么要有它：模板嵌入（`src=`）每次渲染都要去读那些模板页，而编辑器预览
/// **每敲一个字**就重渲染一次 —— 不缓存的话，一次编辑就是几十次解密。
/// 键上带着版本号：那一页提交了新版本，缓存自然失效。
static PAGES: LazyLock<Mutex<HashMap<String, (u64, String)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// 给某一版解锁
pub fn unlock(id: &str, rev: u64, passphrase: String) {
    shared().insert((id.to_string(), rev), passphrase);
}

/// 这一版在这次会话里有没有口令
pub fn passphrase_for(id: &str, rev: u64) -> Option<String> {
    shared().get(&(id.to_string(), rev)).cloned()
}

/// 这一页最近解过的那一把口令。
///
/// 草稿用得上：槽位**每篇只有一个**、不分版本，所以它只能沿用最近解开的那一把。
pub fn latest_for(id: &str) -> Option<String> {
    shared()
        .iter()
        .filter(|((key, _), _)| key == id)
        .max_by_key(|((_, rev), _)| *rev)
        .map(|(_, passphrase)| passphrase.clone())
}

/// 忘掉某一版。口令错了就把它丢掉 —— 留着只会让"再输一次"变成不可能。
pub fn forget(id: &str, rev: u64) {
    drop_pages();
    shared().remove(&(id.to_string(), rev));
}

/// 忘掉一整篇（它的每一版）。
///
/// 「我不想让它留着了」—— 与 [`forget`] 不同，那个是口令错了顺手丢掉；
/// 这个是用户明说要忘掉，所以整篇一起清。
pub fn forget_note(id: &str) {
    drop_pages();
    shared().retain(|(key, _), _| key != id);
}

/// 全部忘掉（上锁）
pub fn forget_all() {
    drop_pages();
    shared().clear();
}

fn shared() -> std::sync::MutexGuard<'static, HashMap<Key, String>> {
    PASSWORDS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// 口令是**进程内共享**的：碰它的用例要串行。
///
/// 锁放在这里而不是各个测试模块里 —— 它保护的是**同一份**共享状态，
/// 每个模块各拿一把锁等于没锁（两个模块的用例照样会撞上）。
#[cfg(test)]
pub mod test_lock {
    /// 碰会话口令的用例先拿它
    pub fn guard() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guard() -> std::sync::MutexGuard<'static, ()> {
        test_lock::guard()
    }

    #[test]
    fn forgetting_one_note_leaves_the_others_alone() {
        let _guard = guard();
        forget_all();

        unlock("甲", 1, "甲的口令".to_string());
        unlock("甲", 2, "甲的另一把".to_string());
        unlock("乙", 1, "乙的口令".to_string());

        forget_note("甲");

        assert!(passphrase_for("甲", 1).is_none());
        assert!(passphrase_for("甲", 2).is_none());
        assert!(latest_for("甲").is_none());
        assert_eq!(passphrase_for("乙", 1).as_deref(), Some("乙的口令"));

        forget_all();
    }
}

/// 这一趟里读过的某一页（版本号对得上才算数）
pub fn cached_page(title: &str) -> Option<(u64, String)> {
    PAGES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(title)
        .cloned()
}

/// 记下这一页读出来的样子
pub fn cache_page(title: &str, rev: u64, markdown: String) {
    PAGES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(title.to_string(), (rev, markdown));
}

/// 把读过的内容一并忘掉。
///
/// **必须跟着口令一起忘**：那一页能读出来，是因为这一趟的口令还在；
/// 口令一丢（锁定、换页、清理），留着正文就等于"锁了门还把东西摊在桌上"。
fn drop_pages() {
    PAGES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clear();
}
