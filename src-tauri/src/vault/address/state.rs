//! 浏览状态那一段：`@` 后面的词 ↔ [`Mode`](super::Mode) 枚举。
//!
//! 两头都是词表，所以这里只做**互认**：写进去的词要能读回来，读到的词也得说得清
//! 自己在地址里长什么样。`view` 是唯一可以缩写掉的状态（裸地址就是"看最新版"）。

use super::{Address, Mode};

/// `@` 后面那一整段 → `Mode`。
///
/// 状态词不分大小写；`-` 后面是 **token**，原样收下（`@View-ABC` 的 token 是 `ABC`）。
pub(super) fn mode_of(state: &str) -> Result<Mode, String> {
    let raw = state.trim();
    if raw.is_empty() {
        return Ok(Mode::View { reference: None });
    }

    // `no-command` 自己带一个连字符，必须在拆 `词-参数` 之前认出来
    if raw.eq_ignore_ascii_case("no-command") {
        return Ok(Mode::NoCommand);
    }

    let (keyword, reference) = match raw.split_once('-') {
        Some((keyword, reference)) => (keyword, Some(reference.trim())),
        None => (raw, None),
    };

    match (keyword.to_lowercase().as_str(), reference) {
        ("view", None) => Ok(Mode::View { reference: None }),
        // `@view-` 是漏写的版本：它与 `@view`（最新版）不是一回事
        ("view", Some("")) => Err("「@view-」后面要写版本".to_string()),
        ("view", Some(reference)) => Ok(Mode::View {
            reference: Some(reference.to_string()),
        }),
        ("edit", None) => Ok(Mode::Edit),
        ("history", None) => Ok(Mode::History),
        ("delete", None) => Ok(Mode::Delete),
        ("unlock", None) => Ok(Mode::Unlock { reference: None }),
        ("unlock", Some("")) => Err("「@unlock-」后面要写版本".to_string()),
        ("unlock", Some(reference)) => Ok(Mode::Unlock {
            reference: Some(reference.to_string()),
        }),
        // 回退到最新版没有意义，所以必须指明版本
        ("rollback", None | Some("")) => Err("回退要指明版本：@rollback-<版本>".to_string()),
        ("rollback", Some(reference)) => Ok(Mode::Rollback {
            reference: reference.to_string(),
        }),
        // 认不出来的状态要报错，不能静默裁掉 —— 否则打错的 `@edti` 会悄悄变成阅读页
        _ => Err(format!("不认识这个状态：@{raw}")),
    }
}

/// 拼规范串：`[命名空间:]页面名称[@状态][#段落]`，状态排在段落前面。
///
/// 阅读最新版（`Mode::View` 不带 token）缩写掉 `@view` —— 裸名称就是它的规范形状。
pub(crate) fn compose(address: &Address) -> String {
    let mut out = String::new();
    if !address.namespace.spelling.is_empty() {
        out.push_str(&address.namespace.spelling);
        out.push(':');
    }
    out.push_str(&address.page);
    if let Some(state) = state_of(&address.mode) {
        out.push('@');
        out.push_str(&state);
    }
    if !address.section.is_empty() {
        out.push('#');
        out.push_str(&address.section);
    }
    out
}

/// 状态在规范串里的写法；`None` = 缩写掉。
pub(super) fn state_of(mode: &Mode) -> Option<String> {
    match mode {
        Mode::View { reference } => reference.as_ref().map(|token| format!("view-{token}")),
        Mode::Edit => Some("edit".to_string()),
        Mode::History => Some("history".to_string()),
        Mode::Delete => Some("delete".to_string()),
        Mode::Rollback { reference } => Some(format!("rollback-{reference}")),
        Mode::Unlock { reference } => Some(match reference {
            Some(token) => format!("unlock-{token}"),
            None => "unlock".to_string(),
        }),
        Mode::NoCommand => Some("no-command".to_string()),
    }
}
