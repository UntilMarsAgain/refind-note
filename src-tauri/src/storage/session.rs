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

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex};

type Key = (String, u64);

static PASSWORDS: LazyLock<Mutex<HashMap<Key, String>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// 这一趟里已经读出来的**页正文**：标题 → （版本号, 正文）。
///
/// 为什么要有它：模板嵌入（`src=`）每次渲染都要去读那些模板页，而编辑器预览
/// **每敲一个字**就重渲染一次 —— 不缓存的话，一次编辑就是几十次解密。
/// 键上带着版本号：那一页提交了新版本，缓存自然失效。
static PAGES: LazyLock<Mutex<HashMap<String, CachedPage>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// 缓存里的一页。`stamp` 是**代际标签**，见 [`PAGE_ORDER`]。
struct CachedPage {
    rev: u64,
    markdown: String,
    /// 第几次被写进来（全局递增，见 [`PAGE_CLOCK`]）
    stamp: u64,
}

/// 循环队列：按"被写进来"的先后排队，队头就是最旧的一页。
///
/// 为什么需要它而不能直接扫 map 找最小 `stamp`：那样每次驱逐都要遍历
/// （当然 64 项也不贵）。队列的好处是**均摊 O(1)**，而且它天然记住了顺序，
/// 不用给 map 的每个键再挂一份序号。
///
/// 它存的是 `(标题, 代际)`：同一页被重写时 map 里的 `stamp` 会更新，
/// 而队列里**旧的那一条还在**。弹出时比对 `stamp` —— 对不上说明那是过期记录，
/// 直接丢掉（真正的那一条还在队列后面）。这就是"代际标签"的用处。
static PAGE_ORDER: LazyLock<Mutex<VecDeque<(String, u64)>>> =
    LazyLock::new(|| Mutex::new(VecDeque::new()));

/// 代际计数器：每写进一页就 +1，于是 `stamp` 的大小就是"新旧的次序"。
static PAGE_CLOCK: AtomicU64 = AtomicU64::new(0);

/// 页缓存里最多留几页。超了**逐出最旧的一页**。
///
/// 逐出而不是整份清掉：整份清掉会让"仓库里模板页很多、来回翻页"这种用法
/// 周期性地产���一次解密风暴 —— 63 页明明还热着却全被扔了。
const PAGE_CACHE_LIMIT: usize = 64;

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

    /// 缓存**上限**盯的是两件不同的事，别混：
    ///
    /// 1. **同一页**再读一遍（它提交了新版本）—— `HashMap::insert` 按 key
    ///    覆盖，**长度不变**。这是"新的覆盖旧的"。
    /// 2. **不同页**读到第 64 页 —— 长度要涨，那时才轮到上限。
    #[test]
    fn the_same_page_is_overwritten_rather_than_rejected() {
        let _guard = guard();
        drop_pages();

        cache_page("甲", 1, "第一版".to_string());
        cache_page("甲", 2, "第二版".to_string());

        assert_eq!(
            cached_page("甲"),
            Some((2, "第二版".to_string())),
            "同一页的新版本应当直接覆盖旧的，而不是被拒之门外"
        );
        assert_eq!(cached_len(), 1, "覆盖不增长");
        drop_pages();
    }

    /// 上限到了会发生什么：整份清掉再放新的（不是"拦住"，也不是逐出最旧的一页）。
    ///
    /// 这么定的理由见 `PAGE_CACHE_LIMIT` 那段：这层只为一个目的存在 ——
    /// 让编辑器预览不必每敲一个字就重新解密几十页模板页。
    #[test]
    fn the_page_cache_never_grows_past_its_limit() {
        let _guard = guard();
        drop_pages();

        for index in 0..(PAGE_CACHE_LIMIT * 3) {
            cache_page(&format!("第{index}页"), 1, "正文".to_string());
            assert!(
                cached_len() <= PAGE_CACHE_LIMIT,
                "塞到第 {index} 页时已经有 {} 页了",
                cached_len()
            );
        }
        drop_pages();
    }

    /// 逐出的是**最旧的那一页**，其余热页应当还在 ——
    /// 这是"逐出"与"整份清掉"的分别：后者会把 63 页仍热着的也一起扔了，
    /// 于是"模板页很多、来回翻页"时周期性地产��一次解密风暴。
    #[test]
    fn evicting_takes_only_the_oldest_page() {
        let _guard = guard();
        drop_pages();

        for index in 0..PAGE_CACHE_LIMIT {
            cache_page(&format!("第{index}页"), 1, "正文".to_string());
        }
        assert_eq!(cached_len(), PAGE_CACHE_LIMIT);

        // 第 64 页挤进来了 → 只有"第0页"该被逐出
        cache_page("新来的", 1, "正文".to_string());

        assert_eq!(cached_len(), PAGE_CACHE_LIMIT, "仍然不超上限");
        assert!(cached_page("第0页").is_none(), "最旧的第0页该被逐出");
        assert!(cached_page("新来的").is_some(), "刚写进来的那页不该被逐出");
        assert!(
            cached_page(&format!("第{}页", PAGE_CACHE_LIMIT - 1)).is_some(),
            "其余热页都该还在 —— 整份清掉的话这里会 None"
        );
        drop_pages();
    }

    /// 同一页被反复重写时，队列里会堆下一串**过期记录**。
    /// 驱逐必须跳过它们，否则会把刚写进来的那一页误当成最旧的逐出去。
    #[test]
    fn evicting_skips_the_stale_queue_entries_of_one_rewritten_page() {
        let _guard = guard();
        drop_pages();

        // 填满上限，然后对**同一页**重写很多次 —— 队列里于是全是它的旧记录
        for index in 0..PAGE_CACHE_LIMIT {
            cache_page(&format!("第{index}页"), 1, "正文".to_string());
        }
        for round in 0..(PAGE_CACHE_LIMIT * 2) {
            cache_page("反复重写的那页", 1, format!("第{round}次"));
        }

        assert!(
            cached_page("反复重写的那页").is_some(),
            "刚重写的这一版不该被过期记录挤掉"
        );
        assert_eq!(cached_len(), PAGE_CACHE_LIMIT, "仍不超上限");
        drop_pages();
    }

    fn cached_len() -> usize {
        PAGES
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len()
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
    let pages = PAGES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    pages
        .get(title)
        .map(|entry| (entry.rev, entry.markdown.clone()))
}

/// 记下这一页读出来的样子
///
/// 返回 `true`：留着给调用方按"我确实记下了"用（现在没有调用方需要，
/// 但删掉这个返回值就等于把"记下了吗"这个问题留给以后重新踩一遍）。
pub fn cache_page(title: &str, rev: u64, markdown: String) -> bool {
    // 代际标签：全局递增，于是 `stamp` 的大小就是"谁更新"
    let stamp = PAGE_CLOCK.fetch_add(1, Ordering::Relaxed);

    PAGES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(
            title.to_string(),
            CachedPage {
                rev,
                markdown,
                stamp,
            },
        );

    // 循环队列：每次写进都排在队尾，于是队头就是最旧的一页
    page_order().push_back((title.to_string(), stamp));

    evict_beyond_limit();
    true
}

/// 队列上那把锁。
///
/// 单开一个函数是因为要容错（中毒了的锁取 `into_inner`），而它有三四处用到。
fn page_order() -> std::sync::MutexGuard<'static, VecDeque<(String, u64)>> {
    PAGE_ORDER
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// 超过上限就逐出**最旧的一页**。
///
/// 队头那条**可能已经过期**：同一页被重写时 map 指向了新代际，而队列里旧的那条还在。
/// 比对 `stamp` 认出来就丢掉它继续看下一个 —— 真正的那一条排在后面。
/// 这就是"代际标签"的用处：没有它，同一个标题的过期记录会被当成最旧的逐出去，
/// 把刚写进来的那一页误伤。
fn evict_beyond_limit() {
    loop {
        if PAGES
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len()
            <= PAGE_CACHE_LIMIT
        {
            return;
        }

        // 队列空了还超限：不该发生（map 的每一项都该在队列里），保守退出而不是空转
        let Some((title, stamp)) = page_order().pop_front() else {
            return;
        };

        let mut pages = PAGES
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if pages.get(&title).map(|entry| entry.stamp) == Some(stamp) {
            // 不是过期记录，这就是最旧的那一页，逐出去
            pages.remove(&title);
            return;
        }
        // 过期记录：map 里那个标题已经指向更新的代际，丢掉这条继续看队头
    }
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
    // 队列必须一起清：只清 map 的话，队列里还留着那些标题与代际 ——
    // 下一批页面进来时会和这些残留记录交错，驱逐就会先弹出一串"过期记录"
    // （逻辑上不算错，但队列会莫名长出比 map 还多的条目）。
    page_order().clear();
}
