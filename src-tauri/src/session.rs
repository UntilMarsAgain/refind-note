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
    shared().remove(&(id.to_string(), rev));
}

/// 全部忘掉（上锁）
pub fn forget_all() {
    shared().clear();
}

fn shared() -> std::sync::MutexGuard<'static, HashMap<Key, String>> {
    PASSWORDS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
