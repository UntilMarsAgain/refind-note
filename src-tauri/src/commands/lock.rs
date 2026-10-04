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

//! 页内解锁：一条命令管住"笔记"与"文件"两种上了锁的份。
//!
//! ## 为什么要有这一条
//!
//! 页内读不出来的份有两条来路 —— 模板页（`::decrypt` 那个框）与加密附件
//! （`::image`/`![…]` 换成的那个框），而它们原本是两套：后一套在前端手工搭 DOM，
//! 提示文案写在 TS 里；前一套在后端渲染，文案写在 Rust 里。行为于是分了家。
//!
//! 更要紧的是**它们各自都没验证过解锁成功**。口令层还好：`unlock` 当场验，
//! 错的会抛出来，框留在原地（见 `vault::resolve::revisions::unlock` 为什么那么写）。
//! 但 **gpg 那一层没有口令**，于是没有任何一步说话 —— 点"显示"、换掉框、
//! `<img>` 去拉、`platform::protocol` 把解密失败**和"没有这一份"一起**答成 404、
//! 前端于是说"图片不存在"。框已经没了，人既不能重试也看不到按钮。
//!
//! 所以这里的关键是 [`LockState::readable`]：它不是**推断**出来的，是**真的去读了一次**
//! 才知道的。没有口令的那一层，只有真读才会说话。
//!
//! ## 形状为什么两边一样
//!
//! 返回的字段对两种份都成立，于是前端的解锁框只有**一个**实现
//! （`src/dom/decrypt.ts` 的 `decryptBox`），差别只在 `kind` 决定解锁之后怎么刷新：
//! 文件换一个元素就行，模板页得整页重渲染（模板内容是渲染期烤进 HTML 的）。
//!
//! `reason` 是给**人**看的，所以走原文，不做归类 —— "没有私钥"与"代理被取消了"
//! 该分别说，不该都被压成"解密失败"。

use serde::Serialize;

use crate::open_database;
use crate::vault::database::Database;

/// 页内解锁框的种类
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// 一篇笔记（含被 `::` 引用的模板页）
    Page,
    /// 一个文件页面（图片、音视频、附件）
    File,
}

impl Kind {
    /// 认这个字符串；认不出就是 `None`，由调用方决定怎么报错
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "page" | "note" => Some(Kind::Page),
            "file" => Some(Kind::File),
            _ => None,
        }
    }
}

/// 一份东西现在的锁状态
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LockState {
    pub kind: Kind,
    /// 页内解锁框拿它去重新问（`unlock` 与 `read` 都按标题走）
    pub title: String,
    /// 仓库里有没有这一份。为假时后面几个字段都没意义
    pub exists: bool,
    /// 要不要先解锁才能读
    pub needs_unlock: bool,
    /// 需要口令（对称层）。gpg 那一层为假 —— 它问的是钥匙串
    pub needs_passphrase: bool,
    /// gpg 加密的（有没有私钥真要读的时候才知道）
    pub needs_secret_key: bool,
    /// 口令正躺在本次会话里
    pub passphrase_ready: bool,
    /// **真的去读过了，读得动吗**
    pub readable: bool,
    /// 读不动时为什么（原文，给人看）
    pub reason: String,
}

impl LockState {
    /// 仓库里压根没有这一份 —— 与"上了锁"是两件事，不能混
    fn missing(kind: Kind, title: &str) -> Self {
        Self {
            kind,
            title: title.to_string(),
            exists: false,
            needs_unlock: false,
            needs_passphrase: false,
            needs_secret_key: false,
            passphrase_ready: false,
            readable: false,
            reason: format!("仓库里没有「{title}」"),
        }
    }
}

/// 页内解锁：**交口令（可省）→ 真的去读一次 → 报告读得动读不动**。
///
/// 这是页内解锁框唯一的出口。两条来路（模板页、文件）都走它，所以框的行为只有一处定义。
///
/// ## 口令错了为什么不 `Err`
///
/// 返回 `Ok` 里 `readable: false` + `reason`，而不是把命令本身判失败：解锁框**要在原地
/// 留下来**，让人改了口令再按一次。命令抛错的话前端只能知道"失败了"，得另外约定
/// 怎么区分"口令错了"与"仓库坏了"。这里让**每次尝试都有结构化的答案**，框就不必猜。
#[tauri::command]
pub fn lock_state(
    kind: String,
    title: String,
    reference: Option<String>,
    passphrase: Option<String>,
) -> Result<LockState, String> {
    let Some(kind) = Kind::parse(&kind) else {
        return Err(format!("不认识的种类：{kind:?}（要 page 或 file）"));
    };
    let (_, database) = open_database()?;

    // 有口令就先交 —— `unlock` **当场验**，错口令不会留在会话里
    // （留着的话"再输一次"这条路就断了，见 revisions.rs 里那段说明）
    if let Some(passphrase) = passphrase.as_deref().filter(|value| !value.is_empty()) {
        database.unlock(&title, reference.as_deref(), passphrase)?;
    }

    match kind {
        Kind::Page => Ok(page_state(&database, &title, reference.as_deref())),
        Kind::File => Ok(file_state(&database, &title, reference.as_deref())),
    }
}

/// 一篇笔记的锁状态
fn page_state(database: &Database, title: &str, reference: Option<&str>) -> LockState {
    // 头（怎么存的、口令在不在）不需要口令 —— 头本来就是明文
    let head = database.note_head(title, reference);
    let mut state = LockState {
        kind: Kind::Page,
        title: title.to_string(),
        exists: database.exists(title),
        needs_unlock: head.as_ref().is_ok_and(|head| head.needs_unlock),
        needs_passphrase: head.as_ref().is_ok_and(|head| head.needs_passphrase),
        needs_secret_key: head.as_ref().is_ok_and(|head| head.needs_secret_key),
        passphrase_ready: head.as_ref().is_ok_and(|head| head.passphrase_ready),
        readable: false,
        reason: String::new(),
    };

    if !state.exists {
        state.reason = format!("仓库里没有「{title}」");
        return state;
    }

    // **真的去读**：gpg 那一层只有到这一步才会说话（有没有私钥、代理答不答应）
    match database.read_note(title, reference) {
        Ok(_) => state.readable = true,
        Err(why) => state.reason = why,
    }
    state
}

/// 一个文件页面的锁状态
fn file_state(database: &Database, title: &str, reference: Option<&str>) -> LockState {
    let info = match database.file_info(title, reference) {
        Ok(info) => info,
        // `file_info` 认不出这个标题：既不是文件也不是笔记
        Err(_) => return LockState::missing(Kind::File, title),
    };

    let mut state = LockState {
        kind: Kind::File,
        title: title.to_string(),
        exists: true,
        // `needs_unlock` 在 `FileEntry` 上（serde flatten 进 `FileInfo`）
        needs_unlock: info.entry.needs_unlock,
        needs_passphrase: info.needs_passphrase,
        needs_secret_key: info.needs_secret_key,
        passphrase_ready: info.passphrase_ready,
        readable: false,
        reason: String::new(),
    };

    match database.read_file(title, reference) {
        Ok(_) => state.readable = true,
        Err(why) => state.reason = why,
    }
    state
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_kind_string_is_read_loosely_but_refuses_the_rest() {
        assert_eq!(Kind::parse("page"), Some(Kind::Page));
        assert_eq!(Kind::parse(" Page "), Some(Kind::Page));
        assert_eq!(Kind::parse("FILE"), Some(Kind::File));
        assert_eq!(Kind::parse("note"), Some(Kind::Page));
        // 认不出的必须给 None，不能悄悄当成某一种
        assert_eq!(Kind::parse("目录"), None);
        assert_eq!(Kind::parse(""), None);
    }

    /// 序列化后的字段名要跟前端 `ipc/lock.ts` 里的一致 —— 改一边就要改另一边
    #[test]
    fn the_state_serializes_to_the_names_the_frontend_expects() {
        let state = LockState::missing(Kind::File, "File:桥.png");
        let json = serde_json::to_string(&state).unwrap();
        for field in [
            "kind",
            "title",
            "exists",
            "needsUnlock",
            "needsPassphrase",
            "needsSecretKey",
            "passphraseReady",
            "readable",
            "reason",
        ] {
            assert!(json.contains(field), "少了 {field}：{json}");
        }
        assert!(json.contains("\"kind\":\"file\""), "{json}");
    }
}
