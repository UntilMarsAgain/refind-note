//! 指令页面的**语法**。
//!
//! 这里只回答一个问题：**这一页是不是指令页面，指令是什么**。
//! 至于"拿到指令之后怎么办"（跟重定向、跳几跳、怎么报错）属于仓库的策略，在
//! [`crate::storage`] 里 —— 语法与执行分开，改语法不必动存储，反之亦然。
//!
//! 语法只有两条：
//!
//! - **第一行**忽略末尾空白后等于 `$$COMMAND$$` → 这一页是指令页面，其余内容是指令；
//! - 第二行写成 `REDIRECT: <内部地址>` → 一条重定向指令。

/// 指令页面的标记：正文**第一行**（忽略末尾空白）等于它，就认为这是一页指令。
const COMMAND_MARKER: &str = "$$COMMAND$$";

/// 目前唯一实现的指令前缀：`REDIRECT: 内部地址`（写在第 2 行）
const REDIRECT_PREFIX: &str = "REDIRECT:";

/// 一页指令的内容（只有第一行是标记时才算）
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// `REDIRECT: <内部地址>`
    Redirect(String),
    /// 是指令页面，但没认出指令。`None` 表示压根没有第二行。
    ///
    /// 带上原文是为了报错时能把"写错的那一行"显示出来 —— 只说"认不出来"没用。
    Unrecognized(Option<String>),
}

/// 识别「指令页面」。返回 `None` 表示这不是指令页面（第一行不是标记）。
pub fn command_of(markdown: &str) -> Option<Command> {
    let mut lines = markdown.lines();
    if lines.next()?.trim_end() != COMMAND_MARKER {
        return None;
    }

    let Some(second) = lines.next() else {
        return Some(Command::Unrecognized(None));
    };
    let second = second.trim_end();

    // 取前 REDIRECT_PREFIX.len() 个**字符**（不是字节）再比较：按字节切中文会 panic，
    // 而这里恰恰是"用户随便写点什么"的地方（本项目已经因此 abort 过一次）。
    let head_end = second
        .char_indices()
        .nth(REDIRECT_PREFIX.len())
        .map(|(index, _)| index)
        .unwrap_or(second.len());
    if !second[..head_end].eq_ignore_ascii_case(REDIRECT_PREFIX) {
        return Some(Command::Unrecognized(Some(second.to_string())));
    }

    Some(Command::Redirect(second[head_end..].trim().to_string()))
}

#[cfg(test)]
mod tests {
    use super::{command_of, Command};

    /// 第一行末尾的空白要忽略
    #[test]
    fn marker_ignores_trailing_whitespace() {
        assert_eq!(
            command_of("$$COMMAND$$   \nREDIRECT: 目标\n"),
            Some(Command::Redirect("目标".to_string()))
        );
    }

    /// 第一行不是标记 → 不是指令页面（哪怕第二行写了 REDIRECT）
    #[test]
    fn needs_the_marker_on_the_first_line() {
        assert_eq!(command_of("前言\nREDIRECT: 目标\n"), None);
    }

    /// 前缀大小写不敏感
    #[test]
    fn redirect_prefix_is_case_insensitive() {
        assert_eq!(
            command_of("$$COMMAND$$\nredirect: 目标\n"),
            Some(Command::Redirect("目标".to_string()))
        );
    }

    /// 只有标记、没有第二行 → 认不出来（`None` 表示"压根没写"）
    #[test]
    fn marker_without_second_line() {
        assert_eq!(
            command_of("$$COMMAND$$\n"),
            Some(Command::Unrecognized(None))
        );
    }

    /// **多字节安全**：第二行以中文开头时不能按字节切
    #[test]
    fn multi_byte_second_line_does_not_panic() {
        assert_eq!(
            command_of("$$COMMAND$$\n重定向到某处\n"),
            Some(Command::Unrecognized(Some("重定向到某处".to_string())))
        );
    }

    /// 目标为空照样解析成空串（"这是错误"由上层判定，解析层不替它决定）
    #[test]
    fn redirect_without_target_parses_as_empty() {
        assert_eq!(
            command_of("$$COMMAND$$\nREDIRECT:\n"),
            Some(Command::Redirect(String::new()))
        );
    }
}
