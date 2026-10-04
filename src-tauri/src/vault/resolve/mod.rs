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

//! 把一个地址**落到仓库上**：这是哪一页、能不能改、是第几版。
//!
//! 语法（前缀、状态词、章节、`@` 后面的版本 token）在 [`address`] 里；这一层回答的是
//! **落点**问题 —— 这一页在不在、是哪种地方（仓库里的笔记 / 还不存在的页 / 特殊页 /
//! 帮助页 / 跨站页 / 文件页）、界面该不该摆编辑按钮、版本 token 是第几版。两个子模块
//! 各管一半：
//!
//! - [`pages`]：地址 → **哪一页**。特殊页与跨站页的裁剪、指令页的跟跳、随机跳转都归它，
//!   它们都是"地址栏里那行字落到哪个页面上"的问题；
//! - [`revisions`]：**哪一版** → 正文。读、导出、回滚、解锁、落盘封装的逐层报告归它，
//!   它们都只在版本 token 已经定下来之后才发生。
//!
//! 边界很硬：**这一层不生成 HTML，也不碰字节。** 渲染在 `markdown` 那边，加解密与
//! 封装细节在 `storage` 那边；这里只把"落到哪一页、哪一版"算清楚，正文读不读得动
//! （含上锁状态）由 [`Database::read_note`] 交给 [`crate::vault::notes`] 说话。
//!
//! 也就是说：解析出来的地址 + 当前仓库的内容 = 一个 [`Outcome`]。界面只认这个结论，
//! 不自己再猜一遍"这一页该不该能改"。

mod export_format;
mod pages;
mod revisions;

pub use export_format::ExportFormat;

#[cfg(test)]
mod tests;

use serde::Serialize;

use crate::storage::codec::{self, EncryptionReport, SignatureReport};
use crate::vault::address::Address;
use crate::vault::command;
use crate::vault::database::Database;

/// 地址落到仓库上的结论："这是什么地方"
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Outcome {
    /// 仓库里有这一页
    Note { title: String },
    /// 仓库里还没有这一页（交给"创建"那条路）
    Missing { title: String },
    /// 特殊页面（虚拟命名空间）：前端按 `page` 选视图
    Special { page: String },
    /// 帮助页（虚拟命名空间 `Help`）：页面随程序发布，不在仓库里
    Help { page: String, title: String },
    /// 跨站命名空间里的页面：本仓库没有它，地址在 `url`
    CrossSite { title: String, url: String },
    /// 文件页面（`File:桥.png`）：正文是字节，不是给人读的文本
    File { title: String },
}

/// 这一页是**被哪条指令带过来的**（`$$COMMAND$$` 那一页）
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Via {
    /// 发起跳转的那一页（显示标题）；随机跳转时也会给，界面可以选择不显示
    pub from: String,
    /// 是随机跳转（提示语不写具体名字）
    pub random: bool,
}

/// 跟跳的上限。
///
/// 这是**仓库的跟跳策略**，不是指令语法（语法在 [`crate::vault::command`]）——
/// 指令写成了环时，给一句能看懂的提示，总好过递归到栈溢出。
const MAX_HOPS: usize = 8;

/// 地址 + 它落到仓库上的结论
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ResolvedAddress {
    pub address: Address,
    pub canonical: String,
    pub outcome: Outcome,
    /// **这一页能不能改**。由仓库说了算 —— 界面照它决定摆哪些按钮，
    /// 而不是自己按页面种类特判（帮助页不可改、将来的文件页也不可改）。
    pub editable: bool,
    /// 被指令带过来时才有的"从哪儿来"
    pub via: Option<Via>,
}

/// 某一版落盘封装的细节。三项各自独立，查不动的那项空着（附原因）。
#[derive(Debug, Clone, Default, Serialize)]
pub struct ProtectionReport {
    /// 签名层的校验结论；没有签名层、或验签这步没跑起来时是 `None`
    pub signature: Option<SignatureReport>,
    /// 签名没报出来的原因（没有 gpg、外层口令没给）
    pub signature_problem: Option<String>,
    /// 加密层：加密到谁、本机有没有那把私钥
    pub encryption: Option<EncryptionReport>,
    /// 口令层：这次会话里有没有这一版的口令（没套口令层就是 `None`）
    pub passphrase_ready: Option<bool>,
    /// 压过的话用的哪一档算法（没压过就是 `None`）—— 逐份说，因为同一仓库里可以并存
    pub compression: Option<codec::Compression>,
    /// 套了口令层的话用的哪一档算法（没套就是 `None`）
    pub cipher: Option<codec::Cipher>,
}

/// 把仓库的随机能力交给指令表：指令只知道"要随机挑一篇"，怎么挑是这里的事。
impl command::CommandEnv for Database {
    fn random_title(&self, namespace: Option<&str>, from: &str) -> Result<String, String> {
        self.pick_title(namespace, Some(from))?
            .ok_or_else(|| "那个命名空间里没有别的页面可跳".to_string())
    }
}

/// 版本 token → 版本号（现在只认数字；收 token 是语法层的事，解释 token 是这里的事）
pub(crate) fn token_to_rev(token: &str) -> Result<u64, String> {
    token
        .parse()
        .map_err(|_| format!("版本要写数字（拿到的是「{token}」）"))
}