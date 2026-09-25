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

/// 指令 `REDIRECT: 内部地址`（写在第 2 行）
const REDIRECT_PREFIX: &str = "REDIRECT:";

/// 指令 `RANDOM_REDIRECT[: 命名空间ID]`；不写参数就是在主命名空间里随机
const RANDOM_REDIRECT_PREFIX: &str = "RANDOM_REDIRECT";

/// 一页指令的内容（只有第一行是标记时才算）
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// `REDIRECT: <内部地址>`
    Redirect(String),
    /// `RANDOM_REDIRECT[: 命名空间ID]`。`None` = 主命名空间。
    ///
    /// 命名空间参数保留成字符串：**合法性由上层判定**（它才知道命名空间表长什么样），
    /// 解析层只负责把它切出来。
    RandomRedirect(Option<String>),
    /// 是指令页面，但没认出指令。`None` 表示压根没有第二行。
    ///
    /// 带上原文是为了报错时能把"写错的那一行"显示出来 —— 只说"认不出来"没用。
    Unrecognized(Option<String>),
}

impl Command {
    /// 指令的短名，给界面标注用（`special:all` 据此把它们标出来）。
    ///
    /// 认不出的那一种也要有名字：**它最需要被找出来** —— 这种页面一打开就报错，
    /// 用户得先在列表里看见它，才能进去改。
    pub fn kind(&self) -> &'static str {
        match self {
            Command::Redirect(_) => "redirect",
            Command::RandomRedirect(_) => "random-redirect",
            Command::Unrecognized(_) => "unrecognized",
        }
    }
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

    if let Some(rest) = strip_prefix_ci(second, REDIRECT_PREFIX) {
        return Some(Command::Redirect(rest.trim().to_string()));
    }

    if let Some(rest) = strip_prefix_ci(second, RANDOM_REDIRECT_PREFIX) {
        let rest = rest.trim_start();
        if rest.is_empty() {
            return Some(Command::RandomRedirect(None));
        }
        // 前缀后面必须结束或跟冒号：否则 `RANDOM_REDIRECTX` 也会被当成这条指令
        if let Some(namespace) = rest.strip_prefix(':') {
            let namespace = namespace.trim();
            return Some(Command::RandomRedirect(if namespace.is_empty() {
                None
            } else {
                Some(namespace.to_string())
            }));
        }
        return Some(Command::Unrecognized(Some(second.to_string())));
    }

    Some(Command::Unrecognized(Some(second.to_string())))
}

/// 大小写不敏感的前缀剥离。
///
/// 按**字符**数（不是字节数）取前缀再比较：这里的内容来自用户的正文，按字节切中文
/// 会 panic（本项目已经因此 abort 过一次）。
fn strip_prefix_ci<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let end = text
        .char_indices()
        .nth(prefix.len())
        .map(|(index, _)| index)
        .unwrap_or(text.len());
    text[..end].eq_ignore_ascii_case(prefix).then(|| &text[end..])
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

    /// RANDOM_REDIRECT：可带参数、可不带；冒号后空着 = 主命名空间
    #[test]
    fn random_redirect_forms() {
        assert_eq!(
            command_of("$$COMMAND$$\nRANDOM_REDIRECT\n"),
            Some(Command::RandomRedirect(None))
        );
        assert_eq!(
            command_of("$$COMMAND$$\nRANDOM_REDIRECT: 3\n"),
            Some(Command::RandomRedirect(Some("3".to_string())))
        );
        assert_eq!(
            command_of("$$COMMAND$$\nRANDOM_REDIRECT:\n"),
            Some(Command::RandomRedirect(None))
        );
    }

    /// 前缀后面必须结束或跟冒号：`RANDOM_REDIRECTX` 不是这条指令
    #[test]
    fn random_redirect_needs_a_boundary() {
        assert_eq!(
            command_of("$$COMMAND$$\nRANDOM_REDIRECTX\n"),
            Some(Command::Unrecognized(Some("RANDOM_REDIRECTX".to_string())))
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
