//! 草稿槽位：还没定稿的工作状态。
//!
//! 槽位**一个文档一个**、可覆盖 —— 它不是历史的一版，所以不占版本号、不进 blob 仓。
//! 封装照这篇笔记当前的保护，但**不签名**：签名是提交那一刻的事，草稿签了等于把半成品
//! 也算进作者名下。

use std::fs;

use super::{body_meta, modified_at,
            Draft};
use crate::storage::codec::Secrets;
use crate::storage::session;
use crate::storage::workspace::write_bytes;
use crate::vault::database::Database;

impl Database {
    // ------------------------------------------------------------ 草稿槽位

    /// 存草稿。**一个文档一个槽位**，直接覆盖。
    ///
    /// 封装照**这篇笔记当前的保护**，但**不签名**（签名留给提交）；也不占版本、不进 blob 仓 ——
    /// 草稿是高频写，进仓就变成"每三秒产生一个不可变对象"。
    pub fn save_draft(&self, title: &str, markdown: &str) -> Result<(), String> {
        let id = self
            .id_of(title)
            .ok_or_else(|| format!("没有这篇笔记：{title}"))?;
        let passphrase = session::latest_for(&id);

        // 草稿的**加密**不低于这篇笔记当前的保护（不然那些层就白做了）；签名除外 ——
        // 签名是提交的出处证明，草稿只是本地工作态，自动保存每次动 gpg
        //（还可能弹 pinentry）不值当。
        let mut policy = self.current_policy(&self.state_of(&id)?)?;
        policy.gpg_sign = None;

        let file = crate::storage::codec::encode(
            markdown.as_bytes(),
            &body_meta(),
            &policy,
            passphrase.as_deref(),
        )
        .map_err(|error| error.to_string())?;

        write_bytes(&self.draft_path(&id), &file)
    }

    /// 读草稿。没有就是没有（不是错误）。
    pub fn load_draft(&self, title: &str) -> Result<Option<Draft>, String> {
        let id = self
            .id_of(title)
            .ok_or_else(|| format!("没有这篇笔记：{title}"))?;
        let path = self.draft_path(&id);

        let file = match fs::read(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("读不出 {}：{error}", path.display())),
        };

        let protection = crate::storage::codec::inspect(&file)
            .map_err(|error| error.to_string())?
            .protection;
        let passphrase = session::latest_for(&id);
        let bytes = crate::storage::codec::decode(
            &file,
            &Secrets {
                passphrase: passphrase.as_deref(),
            },
        )
        .map_err(|error| error.to_string())?;

        let markdown =
            String::from_utf8(bytes).map_err(|_| format!("「{title}」的草稿不是文本"))?;

        Ok(Some(Draft {
            markdown,
            modified: modified_at(&path),
            protection,
        }))
    }

    /// 丢掉草稿
    pub fn discard_draft(&self, title: &str) -> Result<bool, String> {
        let id = self
            .id_of(title)
            .ok_or_else(|| format!("没有这篇笔记：{title}"))?;

        match fs::remove_file(self.draft_path(&id)) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(format!("删不掉草稿：{error}")),
        }
    }
}
