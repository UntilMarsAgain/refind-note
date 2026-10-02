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

//! GPG 密钥：列出来、挑一把、导入与删除。
//!
//! 只做"看得见、选得中"这两件事：**私钥永远不会离开钥匙串**，这里也不生成密钥
//! （那要用 `gpg --quick-generate-key` 那套交互流程，不归这个程序管）。
//! 读到的都是钥匙串里本来就有的公开信息：指纹、用户标识、信任程度、能不能签/加密。
//!
//! **手机上这一页是空的**：gpg 是桌面上的系统组件（还要 libgpgme），Android / iOS
//! 上装不了。那几个函数在移动端换成桩（列表空着、导入删除说一句"没有"），
//! 其余功能一切照常 —— 与 [`crate::storage::codec`] 里"系统 gpg"那一段是同一个道理。

use serde::Serialize;

#[cfg(desktop)]
use crate::storage::codec::{gpg_available, gpg_context};

/// 一把钥匙的公开信息
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GpgKey {
    /// 指纹（全大写十六进制）
    pub fingerprint: String,
    /// 用户标识（姓名 <邮箱> 那几行，主标识在前）
    pub uids: Vec<String>,
    /// 本地对它的信任程度（人话）
    pub trust: String,
    /// 有没有私钥（有才签得了、解得开）
    pub secret: bool,
    pub can_sign: bool,
    pub can_encrypt: bool,
    /// 创建时间（RFC3339）；读不出来就是空串
    pub created: String,
    /// 过期时间（RFC3339）；空串 = 永不过期
    pub expires: String,
    /// 已经过期了
    pub expired: bool,
}

/// 导入的结果
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ImportSummary {
    /// 新引进来的钥匙数
    pub imported: u32,
    /// 没有变化的
    pub unchanged: u32,
    /// 其中**带来私钥**的（新收的）
    pub secret_imported: u32,
    /// 私钥部分没有变化的
    pub secret_unchanged: u32,
}

/// 列出钥匙串里的全部钥匙（新的在前）。
///
/// 没有 gpg 的机器上返回空列表 —— 这不是错误：那一页本来就该显示"这台机器上没有 gpg"。
#[cfg(desktop)]
pub fn list() -> Result<Vec<GpgKey>, String> {
    if !gpg_available() {
        return Ok(Vec::new());
    }

    let mut context = gpg_context().map_err(|error| error.to_string())?;

    // 这台机器上**有私钥**的那些指纹。
    //
    // 不能看公开列举里那个 `secret` 标志：实测它一直是 `false`（gpgme 的公开列举不填它，
    // 尽管 `sec:` 明明在钥匙环里）—— 于是"明明有私钥，程序却说只有公钥"，
    // 连带签名时也选不着自己的钥匙。私钥得**单独问一遍**。
    let with_secret: std::collections::BTreeSet<String> = context
        .secret_keys()
        .map_err(|error| error.to_string())?
        .flatten()
        .map(|key| key.fingerprint().unwrap_or_default().to_string())
        .collect();

    let keys = context.keys().map_err(|error| error.to_string())?;

    let mut out = Vec::new();
    for key in keys.flatten() {
        let fingerprint = key.fingerprint().unwrap_or_default().to_string();
        if fingerprint.is_empty() {
            continue;
        }

        let uids: Vec<String> = key
            .user_ids()
            .filter_map(crate::storage::codec::uid_text)
            .collect();

        // 创建与过期记在**主钥匙**上（`Key` 自己不直接给这两个时间）
        let primary = key.primary_key();
        let created = primary.as_ref().and_then(|sub| sub.creation_time());
        let expires = primary.as_ref().and_then(|sub| sub.expiration_time());

        // 先问一遍再交出去：`fingerprint` 这一行要挪进结构体里
        let has_secret = with_secret.contains(&fingerprint);

        out.push(GpgKey {
            fingerprint,
            uids,
            // 信任看的是**主人**对这把钥匙的判定（自己签的、还是陌生人给的）
            trust: crate::storage::codec::trust_label(key.owner_trust()),
            secret: has_secret,
            can_sign: key.can_sign(),
            can_encrypt: key.can_encrypt(),
            created: stamp(created),
            expires: stamp(expires),
            expired: key.is_expired(),
        });
    }

    Ok(out)
}

/// 从一份钥匙文件（导出的 `.asc` / `.gpg`）导入。
///
/// 文件里带不带私钥都收：gpg 自己分得清，这里照它的结果如实报出来
/// （见 [`ImportSummary::secret_imported`]）—— 带私钥的导入要让人知道。
#[cfg(desktop)]
pub fn import(bytes: &[u8]) -> Result<ImportSummary, String> {
    let mut context = gpg_context().map_err(|error| error.to_string())?;
    let result = context
        .import(bytes)
        .map_err(|error| format!("导入失败：{error}"))?;

    Ok(ImportSummary {
        imported: result.imported(),
        unchanged: result.unchanged(),
        // 私钥是单独一栏：导进来的东西里带了私钥，这件事必须说给人听 ——
        // 它意味着从此这台机器能替那个人签名、解密
        secret_imported: result.secret_imported(),
        secret_unchanged: result.secret_unchanged(),
    })
}

/// 删掉一把**公钥**。
///
/// 有私钥的钥匙串一律不动：那多半是这个人自己的钥匙（或者是别人给他的私钥），
/// 误删的代价太大。要删私钥请自己用 gpg 去删。
#[cfg(desktop)]
pub fn delete(fingerprint: &str) -> Result<(), String> {
    let mut context = gpg_context().map_err(|error| error.to_string())?;
    let key = context
        .get_key(fingerprint)
        .map_err(|error| format!("找不到这把钥匙：{error}"))?;

    if key.has_secret() {
        return Err("这把钥匙带着私钥，程序不替你删 —— 用 gpg 自己删".to_string());
    }
    context
        .delete_key(&key)
        .map_err(|error| format!("删不掉：{error}"))
}

// ---------------------------------------------------------------- 移动端

/// 移动端：没有 gpg，钥匙串就是空的（那一页本来也就该这么显示）
#[cfg(mobile)]
pub fn list() -> Result<Vec<GpgKey>, String> {
    Ok(Vec::new())
}

/// 移动端：这台设备上没有 gpg 可用
#[cfg(mobile)]
pub fn import(_bytes: &[u8]) -> Result<ImportSummary, String> {
    Err(MOBILE_MESSAGE.to_string())
}

/// 移动端：这台设备上没有 gpg 可用
#[cfg(mobile)]
pub fn delete(_fingerprint: &str) -> Result<(), String> {
    Err(MOBILE_MESSAGE.to_string())
}

#[cfg(mobile)]
const MOBILE_MESSAGE: &str = "这台设备上没有 gpg（GPG 那一层是桌面上的功能）";

/// 时间戳 → RFC3339；没有就是空串
#[cfg(desktop)]
fn stamp(at: Option<std::time::SystemTime>) -> String {
    let Some(at) = at else {
        return String::new();
    };
    time::OffsetDateTime::from(at)
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **有私钥就要说"有"** —— 这里踩过一次：公开列举里那个 `secret` 标志一直是 false
    /// （gpgme 不填它），于是"明明有私钥，页面说只有公钥、签名也选不着自己的钥匙"。
    ///
    /// 所以这条测的是：凡是 `secret_keys()` 认得出来的指纹，`list()` 也必须说 `secret`。
    #[test]
    fn a_key_with_a_secret_is_reported_as_such() {
        if !crate::storage::codec::gpg_available() {
            return; // 没有 gpg 的机器上这条不算数
        }

        let mut context = gpg_context().unwrap();
        let mine: Vec<String> = context
            .secret_keys()
            .unwrap()
            .flatten()
            .map(|key| key.fingerprint().unwrap_or_default().to_string())
            .collect();
        if mine.is_empty() {
            return; // 钥匙环里本来就没有私钥，没什么可钉的
        }

        let listed = list().unwrap();
        for fingerprint in mine {
            let found = listed
                .iter()
                .find(|key| key.fingerprint == fingerprint)
                .unwrap_or_else(|| panic!("{fingerprint} 应当出现在列表里"));
            assert!(found.secret, "{fingerprint} 有私钥，就该报 true");
        }
    }

    /// 钥匙串是**进程外**的东西，测试不去动它 —— 这里只钉住"没有 gpg 时不炸"。
    #[test]
    fn without_gpg_the_list_is_simply_empty() {
        // 有 gpg 的机器上这条不算数：它测的是"没有的那条路"
        if crate::storage::codec::gpg_available() {
            return;
        }
        assert_eq!(list().unwrap(), Vec::new());
    }
}
