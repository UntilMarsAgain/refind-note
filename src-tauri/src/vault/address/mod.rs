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

//! 地址：`命名空间:页面名称@浏览状态#段落` 的解析与规范拼写。
//!
//! 这一层只做**语法**：前缀、状态词、章节怎么读，页面名怎么规整、哪些字符不合法。
//! 页面在不在、是哪一版 —— 一切要看仓库内容的问题都不归这里管。
//!
//! 解析产物 `Address` 是地址的**身份**：稳定、可比较、可持久化；`canonical`
//! 是它对应的规范串（`compose` 的结果），地址栏回显、历史、剪贴板都用它 ——
//! 前端不自己拼地址字符串。
//!
//! ## 这一层怎么分的
//!
//! 一条地址要拆成三段才能读，于是按那几段分文件：
//!
//! | 文件 | 管什么 |
//! |---|---|
//! | 本文件 | 地址的**身份**（`Address` 等类型）与对外的 `parse` |
//! | [`split`] | 怎么把一个字符串切成几段 —— 纯切分，不问合不合法 |
//! | [`state`] | 浏览状态那一段：`@` 后面的词 ↔ [`Mode`] 枚举 |
//!
//! 页面名怎么规整、哪些字符不合法，统统是 [`crate::vault::title`] 的事 ——
//! 这一层只管分段与状态词。

mod split;
mod state;
#[cfg(test)]
mod tests;

// `compose` 住在 `state` 里（它与状态词是同一件事的两头），但调用方一直写
// `address::compose` —— 那个路径不能因为拆文件而变，所以在这里重导出。
pub(crate) use state::compose;

use serde::Serialize;

use crate::vault::namespace::{NamespaceTable, HELP_ID, SPECIAL_ID};
use crate::vault::title;

use split::{split_address, AddressParts};
use state::mode_of;

/// 现有的特殊页面。不在这里面的 `special:` 地址直接报「没有这个特殊页面」。
pub const SPECIAL_PAGES: [&str; 11] = [
    "newtab", "settings", "all", "random", "debug", "trash", "gc", "files", "changes", "history",
    "keys",
];

/// 命名空间部分：`id` 是它的身份，`spelling` 是回显时用的拼写。
///
/// 主命名空间没有前缀，两者都是空串。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NamespaceRef {
    pub id: String,
    pub spelling: String,
}

/// 浏览状态：`@` 后面那一段。
///
/// `reference` 收下的只是 **token**：它是版本号还是别的引用形式，由仓库那一层解释。
/// `None` 表示这一种状态不指版本 —— 阅读 = 最新版，解锁 = 最新版。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Mode {
    /// 阅读：`None` 是最新版（规范串里缩写掉 `@view`），`Some` 是 `@view-<token>`
    View {
        #[serde(rename = "ref")]
        reference: Option<String>,
    },
    Edit,
    History,
    Delete,
    /// 回退：必须指明版本（`@rollback-<token>`）
    Rollback {
        #[serde(rename = "ref")]
        reference: String,
    },
    /// 等待口令：`@unlock`（最新版）或 `@unlock-<token>`
    Unlock {
        #[serde(rename = "ref")]
        reference: Option<String>,
    },
    /// `@no-command`：这一页是指令页，但**不跟跳**，照原文看
    NoCommand,
}

/// 一个地址：`命名空间:页面名称@浏览状态#段落`。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Address {
    pub namespace: NamespaceRef,
    /// 页面名。规整过：`_` 视作空格、空白折叠、英文首字母大写
    pub page: String,
    pub mode: Mode,
    /// 章节（段落）。空串 = 没写
    pub section: String,
}

/// 一次解析的产物：地址本身 + 它的规范串。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ParsedAddress {
    pub address: Address,
    /// 规范串：地址栏回显、历史都用它
    pub canonical: String,
}

/// 解析地址栏那一行：空输入不是地址（`None`）；语法有问题时，错误里是一句给人看的话。
///
/// 要那张命名空间表：`帮助:入门` 说的到底是哪个命名空间，是表说了算 ——
/// 所以"语法"这一层也得看得见它。
pub fn parse(input: &str, table: &NamespaceTable) -> Result<Option<ParsedAddress>, String> {
    let raw = input.trim();
    if raw.is_empty() {
        return Ok(None);
    }

    let AddressParts {
        name,
        state,
        section,
    } = split_address(raw);
    let name = title::normalize(&name);

    // 冒号只可能属于命名空间前缀。认不出来就报错，**不回退主命名空间** ——
    // MediaWiki 那套（未知前缀一律当主命名空间的标题）到这里是打错的字，
    // 静默收下只会让人以为"这篇笔记没了"。
    if let Some((prefix, rest)) = name.split_once(':') {
        let prefix = prefix.trim();
        if prefix.is_empty() {
            return Err(title::reject_namespace(prefix, table));
        }
        let Some(found) = table.lookup(prefix) else {
            return Err(title::reject_namespace(prefix, table));
        };

        if found.storable {
            let address = Address {
                namespace: NamespaceRef {
                    id: found.id.clone(),
                    // 回显用**写下来的那个拼写**：用别名访问就用别名还回去
                    spelling: prefix.to_string(),
                },
                page: title::check_page(rest.trim())?,
                mode: mode_of(state.as_deref().unwrap_or(""))?,
                section: section.unwrap_or_default(),
            };
            let canonical = compose(&address);
            return Ok(Some(ParsedAddress { address, canonical }));
        }

        if found.id == SPECIAL_ID {
            return special(rest.trim(), section, table).map(Some);
        }
        // 跨站命名空间：页面在别人家，但它**是个地址** —— 写成地址就得解析得出来，
        // 落到仓库上会得到"交给浏览器打开"那个结论（见 resolve）
        if found.is_cross_site() {
            let address = Address {
                namespace: NamespaceRef {
                    id: found.id.clone(),
                    spelling: prefix.to_string(),
                },
                page: title::check_page(rest.trim())?,
                mode: mode_of(state.as_deref().unwrap_or(""))?,
                section: section.unwrap_or_default(),
            };
            let canonical = compose(&address);
            return Ok(Some(ParsedAddress { address, canonical }));
        }
        if found.id == HELP_ID {
            // 帮助页：页面名在**编译进来的那张表**里，这里只做词法检查，
            // 在不在由 resolve 那一层回答（它才拿得到仓库）
            let address = Address {
                namespace: NamespaceRef {
                    id: HELP_ID.to_string(),
                    spelling: prefix.to_string(),
                },
                page: title::check_page(rest.trim())?,
                mode: mode_of(state.as_deref().unwrap_or(""))?,
                section: section.unwrap_or_default(),
            };
            let canonical = compose(&address);
            return Ok(Some(ParsedAddress { address, canonical }));
        }
        // 剩下的只可能是"不可存储、又不是特殊/帮助/跨站"的命名空间 ——
        // 内建的三个都不落这一支，留着是给以后新加的命名空间一个说得清的错
        return Err(format!("「{}」不是能写成地址的命名空间", found.name));
    }

    let address = Address {
        namespace: NamespaceRef {
            id: crate::vault::namespace::MAIN_ID.to_string(),
            spelling: String::new(),
        },
        page: title::check_page(&name)?,
        mode: mode_of(state.as_deref().unwrap_or(""))?,
        section: section.unwrap_or_default(),
    };
    let canonical = compose(&address);
    Ok(Some(ParsedAddress { address, canonical }))
}

/// `special:` 前缀：页面标识按小写查注册表，规范串里写成 `Special:Settings`。
///
/// 特殊页面没有版本、编辑、删除这类状态：`@` 后面写了什么一律裁掉，章节保留 ——
/// 地址栏回显出来的规范形状（少了那一段）本身就是提示。
fn special(
    page: &str,
    section: Option<String>,
    table: &NamespaceTable,
) -> Result<ParsedAddress, String> {
    let id = page.to_lowercase();
    if id.is_empty() {
        return Err("special: 后面要写页面名，例如 special:newtab".to_string());
    }
    if !SPECIAL_PAGES.contains(&id.as_str()) {
        return Err(format!(
            "没有这个特殊页面：special:{id}（现有：{}）",
            SPECIAL_PAGES.join("、")
        ));
    }

    let address = Address {
        namespace: NamespaceRef {
            id: SPECIAL_ID.to_string(),
            spelling: title::capitalize_first(&table.name_of(SPECIAL_ID)),
        },
        page: title::capitalize_first(&id),
        mode: Mode::View { reference: None },
        section: section.unwrap_or_default(),
    };
    let canonical = compose(&address);
    Ok(ParsedAddress { address, canonical })
}
