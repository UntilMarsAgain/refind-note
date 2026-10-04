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

//! 页内解锁：**在 `::decrypt` 上提交之后**调的那一条。
//!
//! ## 这一条为什么非得"真的去加载"
//!
//! 因为**别的任何办法都证明不了它成不成**。三层里只有两层会自己说话：
//!
//! - **口令层**：`unlock` 当场验，错口令会抛出来（见
//!   `vault::resolve::revisions::unlock` 为什么那么写）。所以它一直是好的。
//! - **gpg 层**：没有口令，也没有"先试试看"这回事 —— 它要么被 gpg-agent 悄悄解开
//!   （钥匙的口令有缓存、或者智能卡碰一下就行），要么失败。而失败之前
//!   **什么迹象都没有**。
//! - **渲染期**：这一层**故意不问**（`codec::without_prompting`，见
//!   `vault::notes::read::read_for_embedding`）—— 渲染一篇笔记时弹一个 pinentry
//!   窗口是荒唐的，那一页会被渲染很多次（编辑器每敲一个字一次）。
//!
//! 于是"到底行不行"只能由**人按一下那个按钮**来问。这就是这一条命令存在的理由，
//! 也是 `::decrypt` 上那个按钮为什么不是"改个地址重新导航"那么简单。
//!
//! ## 为什么成功时不返回内容
//!
//! 成功时只说"成了"，由界面自己重读。这样"替换掉那个占位"这件事**每处都做得对**：
//!
//! - 页内嵌着的模板页：重渲染那一篇 —— `::卡片` 的参数是**调用点**给的，
//!   只看模板页本身算不出来（见 `markdown::syntax::template::fill::substitute`）；
//! - 读的那一篇笔记：重读它自己；
//! - 一个附件：换上真的地址。
//!
//! 后端要把这三样都算成一段 HTML 塞回来，就得替三种上下文各写一遍，而模板那一路
//! **注定是错的**。所以分工是：后端负责**说清成不成、为什么不成**（只有它知道），
//! 界面负责**替换**（只有它知道）。

use serde::Serialize;

use crate::open_database;
use crate::storage::codec::Protection;
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
            "page" | "note" | "template" => Some(Kind::Page),
            "file" | "attachment" => Some(Kind::File),
            _ => None,
        }
    }
}

/// 真的加载一次之后的结果
///
/// 字段名是 **snake_case**，与 `ipc/files.ts` 的 `FileInfo` 同一套约定 ——
/// Rust 这侧没有全局 serde rename，谁在这边改成 camelCase，前端就会静默收到
/// `undefined`，而 `undefined` 是假值（"不需要口令"被当成真，框上不出现输入框）。
/// 测试 `the_result_serializes_to_the_names_the_frontend_expects` 钉住这一点。
#[derive(Debug, Serialize)]
pub struct ResolveResult {
    pub kind: Kind,
    /// 页内解锁框拿它去重新问
    pub title: String,
    /// 读成了没有
    pub readable: bool,
    /// 读不成时为什么（**原文，给人看**）
    pub reason: String,
    /// 刚才是"口令不对"（界面据此说"再输一次"，而不是让人对着没反应的输入框发呆）
    pub wrong_passphrase: bool,
    /// 要不要给口令输入框。对称层为真；gpg 层为假 —— 它问的是钥匙串或智能卡
    pub needs_passphrase: bool,
}

/// 在 `::decrypt` 上提交之后：**交口令（可省）→ 真的加载一次 → 成或不成**。
///
/// **口令错了不由这一条抛**：抛了前端只知道"失败了"，得另外约定怎么区分
/// "口令不对，再输一次"与"这份东西坏了，别试了"。所以口令错与 gpg 失败都
/// 按平常返回（`Ok`），由 `reason` 与 `wrong_passphrase` 说清楚 —— 解锁框拿到的
/// 就是那一句人话。
///
/// 真正抛的只有"这个命令自己不成立"那几样：种类不认识、这一页压根不在仓库里。
#[tauri::command]
pub fn resolve_decrypt(
    kind: String,
    title: String,
    reference: Option<String>,
    passphrase: Option<String>,
) -> Result<ResolveResult, String> {
    let Some(kind) = Kind::parse(&kind) else {
        return Err(format!("不认识的种类：{kind:?}（要 page 或 file）"));
    };
    let (_, database) = open_database()?;

    // 有口令就先交 —— `unlock` **当场验**，错口令不会留在会话里
    // （留着的话"再输一次"这条路就断了，见 revisions.rs 里那段说明）
    if let Some(passphrase) = passphrase.as_deref().filter(|value| !value.is_empty()) {
        database.unlock(&title, reference.as_deref(), passphrase)?;
    }

    Ok(match kind {
        Kind::Page => probe_page(&database, &title, reference.as_deref()),
        Kind::File => probe_file(&database, &title, reference.as_deref()),
    })
}

fn fail(kind: Kind, title: &str, reason: String) -> ResolveResult {
    ResolveResult {
        kind,
        title: title.to_string(),
        readable: false,
        reason,
        wrong_passphrase: false,
        needs_passphrase: false,
    }
}

/// 真的读一篇（或一版）笔记
///
/// 走的是**界面那条路**（`read_*_for_display`），不是 `read_note` ——
/// 只有前者会把"解不开"归成 `Locked` 并带上一句人话（`reason`）；
/// `read_note` 给的是一句原始错误，界面拿它拼不出解锁框。
fn probe_page(database: &Database, title: &str, reference: Option<&str>) -> ResolveResult {
    use crate::vault::notes::Reading;

    let reading = match reference {
        Some(token) => match crate::vault::resolve::token_to_rev(token) {
            Ok(rev) => database.read_revision_for_display(title, rev),
            Err(why) => return fail(Kind::Page, title, why),
        },
        None => database.read_for_display(title),
    };

    match reading {
        Ok(Reading::Ready { .. }) => ResolveResult {
            kind: Kind::Page,
            title: title.to_string(),
            readable: true,
            reason: String::new(),
            wrong_passphrase: false,
            needs_passphrase: false,
        },
        Ok(Reading::Locked {
            protection,
            reason,
            wrong_passphrase,
        }) => ResolveResult {
            kind: Kind::Page,
            title: title.to_string(),
            readable: false,
            reason,
            wrong_passphrase,
            needs_passphrase: protection.symmetric,
        },
        // `locate` 之类的失败：这一页压根不在仓库里
        Err(why) => fail(Kind::Page, title, why),
    }
}

/// 真的读一个文件
fn probe_file(database: &Database, title: &str, reference: Option<&str>) -> ResolveResult {
    // 头先读：它决定要不要给口令输入框（**不需要口令**，头是明文）
    let info = match database.file_info(title, reference) {
        Ok(info) => info,
        Err(why) => return fail(Kind::File, title, why),
    };
    let needs_passphrase = info.needs_passphrase;
    let protection: Protection = info.entry.protection;

    match database.read_file(title, reference) {
        Ok(_) => ResolveResult {
            kind: Kind::File,
            title: title.to_string(),
            readable: true,
            reason: String::new(),
            wrong_passphrase: false,
            needs_passphrase,
        },
        Err(why) => {
            let wrong_passphrase = why.contains(crate::storage::codec::WRONG_PASSPHRASE_MESSAGE);
            ResolveResult {
                kind: Kind::File,
                title: title.to_string(),
                readable: false,
                reason: why,
                wrong_passphrase,
                needs_passphrase: protection.symmetric || needs_passphrase,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_kind_string_is_read_loosely_but_refuses_the_rest() {
        assert_eq!(Kind::parse("page"), Some(Kind::Page));
        assert_eq!(Kind::parse(" Page "), Some(Kind::Page));
        assert_eq!(Kind::parse("template"), Some(Kind::Page));
        assert_eq!(Kind::parse("FILE"), Some(Kind::File));
        assert_eq!(Kind::parse("attachment"), Some(Kind::File));
        // 认不出的必须给 None，不能悄悄当成某一种
        assert_eq!(Kind::parse("目录"), None);
        assert_eq!(Kind::parse(""), None);
    }

    #[test]
    fn the_kind_serializes_lowercase_because_the_frontend_dispatches_on_it() {
        assert_eq!(serde_json::to_string(&Kind::File).unwrap(), "\"file\"");
        assert_eq!(serde_json::to_string(&Kind::Page).unwrap(), "\"page\"");
    }

    /// 序列化后的字段名要跟前端 `ipc/decrypt.ts` 里的一致。
    ///
    /// 这一条钉的是 **snake_case**：谁在这边改成 camelCase，前端就会静默收到
    /// `undefined`，而 `undefined` 是假值 —— 于是"不需要口令"被当成真，
    /// 框上不会出现输入框，人就卡在那儿了。这个 bug 静默且难查。
    #[test]
    fn the_result_serializes_to_the_names_the_frontend_expects() {
        let result = fail(Kind::File, "File:桥.png", "仓库里没有".to_string());
        let json = serde_json::to_string(&result).unwrap();
        for field in [
            "kind",
            "title",
            "readable",
            "reason",
            "wrong_passphrase",
            "needs_passphrase",
        ] {
            assert!(json.contains(field), "少了 {field}：{json}");
            let camel: String = field
                .split('_')
                .map(|part| {
                    let mut chars = part.chars();
                    match chars.next() {
                        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                        None => String::new(),
                    }
                })
                .collect();
            assert!(!json.contains(&camel), "{field} 变成了 {camel}：{json}");
        }
        assert!(json.contains("\"kind\":\"file\""), "{json}");
    }
}
