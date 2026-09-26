//! 指令的**一张表**。
//!
//! 设计目标只有一句：**加一条新指令，只改这一处**。
//!
//! 一条指令需要的东西全写在同一个 [`CommandSpec`] 里 —— 前缀（怎么写）、短名与中文名
//! （界面怎么标）、参数规则（能不能带冒号参数）、跳到哪（怎么执行）、人话说明（提示怎么写）。
//! 报错时那句"目前支持……"也由表生成，不再手写一份。
//!
//! 语法与执行仍然分开：这里只说"指令要什么"，具体能力（比如"在某命名空间里随机挑一篇"）
//! 由实现方通过 [`CommandEnv`] 提供，所以指令表不必知道仓库长什么样。

/// 指令页面的标记：正文**第一行**（忽略末尾空白）等于它，就认为这是一页指令。
const COMMAND_MARKER: &str = "$$COMMAND$$";

/// 指令执行时需要的仓库能力。
///
/// 指令本身不知道仓库长什么样 —— 它只声明"我要在某个命名空间里随机挑一篇"，
/// 由实现方决定怎么挑。
pub trait CommandEnv {
    /// 在某个命名空间里随机挑一篇笔记的标题（`None` = 主命名空间）。
    /// `from` 是发起这条指令的页面标题（随机时要排除它自己）。
    fn random_title(&self, namespace: Option<&str>, from: &str) -> Result<String, String>;
}

/// 前缀之后怎么读参数。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Argument {
    /// 前缀之后的全部内容就是参数，可以空着
    Rest,
    /// 可选 `: 参数`：前缀之后要么结束、要么跟冒号
    ///
    /// 这一条同时挡住误认：`RANDOM_REDIRECTX` 后面既不是结束也不是冒号，于是不算这条指令。
    OptionalColon,
}

/// 一条指令的全部定义。
#[derive(Debug)]
pub struct CommandSpec {
    /// 写在第二行的前缀（匹配时不区分大小写）
    pub prefix: &'static str,
    /// 界面上的短名
    pub kind: &'static str,
    /// 界面上的中文名
    pub label: &'static str,
    /// 参数规则
    pub argument: Argument,
    /// 这条指令跳到哪。`None` = 不跳（将来会有只产生副作用的指令）
    pub chase: Option<fn(&dyn CommandEnv, &Command, &str) -> Result<String, String>>,
    /// 界面上那句人话说明
    pub describe: fn(&Command) -> String,
}

/// 相等比较只看**表里声明的那些字段**。
///
/// 刻意不派生：`chase` / `describe` 是函数指针，比较它们没有意义（编译器也会警告）。
impl PartialEq for CommandSpec {
    fn eq(&self, other: &Self) -> bool {
        self.prefix == other.prefix
            && self.kind == other.kind
            && self.label == other.label
            && self.argument == other.argument
    }
}

/// 一条认出来的指令：定义 + 原文参数
#[derive(Debug, Clone, PartialEq)]
pub struct Command {
    pub spec: &'static CommandSpec,
    pub argument: String,
}

/// 解析结果
#[derive(Debug, Clone, PartialEq)]
pub enum Parsed {
    /// 不是指令页面（第一行不是标记）
    None,
    /// 是指令页面，但没有第二行
    Empty,
    /// 是指令页面，第二行认不出来（带上那一行原文，报错时要说清是哪一行）
    Unrecognized(String),
    /// 认出来了
    Command(Command),
}

// ------------------------------------------------------------ 指令表本体

/// `REDIRECT: <内部地址>`
static REDIRECT: CommandSpec = CommandSpec {
    prefix: "REDIRECT:",
    kind: "redirect",
    label: "重定向",
    argument: Argument::Rest,
    chase: Some(chase_redirect),
    describe: describe_redirect,
};

/// `RANDOM_REDIRECT[: 命名空间ID]`
static RANDOM_REDIRECT: CommandSpec = CommandSpec {
    prefix: "RANDOM_REDIRECT",
    kind: "random-redirect",
    label: "随机重定向",
    argument: Argument::OptionalColon,
    chase: Some(chase_random_redirect),
    describe: describe_random_redirect,
};

/// **指令总表。加新指令只改这里。**
pub static COMMANDS: &[&CommandSpec] = &[&REDIRECT, &RANDOM_REDIRECT];

/// 报错时给人看的清单。**由表生成** —— 手写第二份必然有一天会与表不符。
pub fn supported() -> String {
    COMMANDS
        .iter()
        .map(|spec| spec.prefix)
        .collect::<Vec<_>>()
        .join("、")
}

// ------------------------------------------------------------ 各指令的执行与说明

fn chase_redirect(_env: &dyn CommandEnv, command: &Command, _from: &str) -> Result<String, String> {
    // "没有目标就是错的"是这条指令自己的要求，写在它旁边
    if command.argument.is_empty() {
        return Err("REDIRECT 没有写目标地址".to_string());
    }
    Ok(command.argument.clone())
}

fn chase_random_redirect(
    env: &dyn CommandEnv,
    command: &Command,
    from: &str,
) -> Result<String, String> {
    let namespace = command.argument.trim();
    let namespace = if namespace.is_empty() {
        None
    } else {
        Some(namespace)
    };
    env.random_title(namespace, from)
}

fn describe_redirect(command: &Command) -> String {
    if command.argument.is_empty() {
        "没有写目标地址".to_string()
    } else {
        // 目标本身由前端渲染成**可点的链接**（见 `CommandInfo::argument`），
        // 所以这里不再重复写进说明文字，也不必用书名号把人名裹起来
        "打开这一页会跳到".to_string()
    }
}

fn describe_random_redirect(command: &Command) -> String {
    let namespace = command.argument.trim();
    // 空与 "0" 都是主命名空间（它没有前缀，用 0 占位）——
    // 界面上不该让人看见那个占位符
    if namespace.is_empty() || namespace == crate::title::MAIN_NS {
        "每次打开随机跳到主命名空间的某一篇".to_string()
    } else {
        format!("每次打开随机跳到命名空间 {namespace} 的某一篇")
    }
}

// ------------------------------------------------------------ 解析

/// 识别「指令页面」。
pub fn parse(markdown: &str) -> Parsed {
    let mut lines = markdown.lines();
    if lines.next().map(str::trim_end) != Some(COMMAND_MARKER) {
        return Parsed::None;
    }

    let Some(second) = lines.next() else {
        return Parsed::Empty;
    };
    let second = second.trim_end();

    for spec in COMMANDS {
        let Some(rest) = strip_prefix_ci(second, spec.prefix) else {
            continue;
        };
        let rest = rest.trim_start();

        match spec.argument {
            Argument::Rest => {
                return Parsed::Command(Command {
                    spec,
                    argument: rest.to_string(),
                });
            }
            Argument::OptionalColon => {
                if rest.is_empty() {
                    return Parsed::Command(Command {
                        spec,
                        argument: String::new(),
                    });
                }
                if let Some(argument) = rest.strip_prefix(':') {
                    return Parsed::Command(Command {
                        spec,
                        argument: argument.trim().to_string(),
                    });
                }
            }
        }
        // 这一条不符（例如 `RANDOM_REDIRECTX`）：交给后面的指令继续试
    }

    Parsed::Unrecognized(second.to_string())
}

impl Parsed {
    /// 界面标注用的短名；`None` 表示这根本不是指令页面
    pub fn kind(&self) -> Option<&'static str> {
        match self {
            Parsed::Command(command) => Some(command.spec.kind),
            // 认不出的也要有名字：**它最需要被找出来**，一打开就报错
            Parsed::Empty | Parsed::Unrecognized(_) => Some("unrecognized"),
            Parsed::None => None,
        }
    }

    /// 界面显示用的中文名
    pub fn label(&self) -> Option<&'static str> {
        match self {
            Parsed::Command(command) => Some(command.spec.label),
            Parsed::Empty | Parsed::Unrecognized(_) => Some("指令有问题"),
            Parsed::None => None,
        }
    }

    /// 认出来的指令（没认出来时没有）
    pub fn command(&self) -> Option<&Command> {
        match self {
            Parsed::Command(command) => Some(command),
            _ => None,
        }
    }

    /// 一句人话说明
    pub fn describe(&self) -> String {
        match self {
            Parsed::Command(command) => (command.spec.describe)(command),
            Parsed::Empty => "写了标记，但没写指令".to_string(),
            Parsed::Unrecognized(line) => format!("认不出来：「{}」", line.trim()),
            Parsed::None => String::new(),
        }
    }

}

impl Command {
    /// 跳到哪。`Ok(None)` = 这条指令不跳。
    pub fn chase(&self, env: &dyn CommandEnv, from: &str) -> Result<Option<String>, String> {
        match self.spec.chase {
            Some(chase) => chase(env, self, from).map(Some),
            None => Ok(None),
        }
    }
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
    use super::*;

    fn parse_ok(markdown: &str) -> Command {
        match parse(markdown) {
            Parsed::Command(command) => command,
            other => panic!("应当认出指令，得到 {other:?}"),
        }
    }

    /// 第一行末尾的空白要忽略
    #[test]
    fn marker_ignores_trailing_whitespace() {
        let command = parse_ok("$$COMMAND$$   \nREDIRECT: 目标\n");
        assert_eq!(command.spec.kind, "redirect");
        assert_eq!(command.argument, "目标");
    }

    /// 第一行不是标记 → 不是指令页面（哪怕第二行写了 REDIRECT）
    #[test]
    fn needs_the_marker_on_the_first_line() {
        assert_eq!(parse("前言\nREDIRECT: 目标\n"), Parsed::None);
    }

    /// 前缀大小写不敏感
    #[test]
    fn redirect_prefix_is_case_insensitive() {
        assert_eq!(parse_ok("$$COMMAND$$\nredirect: 目标\n").argument, "目标");
    }

    /// 只有标记、没有第二行
    #[test]
    fn marker_without_second_line() {
        assert_eq!(parse("$$COMMAND$$\n"), Parsed::Empty);
    }

    /// **多字节安全**：第二行以中文开头时不能按字节切
    #[test]
    fn multi_byte_second_line_does_not_panic() {
        assert_eq!(
            parse("$$COMMAND$$\n重定向到某处\n"),
            Parsed::Unrecognized("重定向到某处".to_string())
        );
    }

    /// RANDOM_REDIRECT：可带参数、可不带；冒号后空着 = 主命名空间
    #[test]
    fn random_redirect_forms() {
        assert_eq!(parse_ok("$$COMMAND$$\nRANDOM_REDIRECT\n").argument, "");
        assert_eq!(parse_ok("$$COMMAND$$\nRANDOM_REDIRECT: 3\n").argument, "3");
        assert_eq!(parse_ok("$$COMMAND$$\nRANDOM_REDIRECT:\n").argument, "");
    }

    /// 前缀后面必须结束或跟冒号：`RANDOM_REDIRECTX` 不是这条指令
    #[test]
    fn random_redirect_needs_a_boundary() {
        assert_eq!(
            parse("$$COMMAND$$\nRANDOM_REDIRECTX\n"),
            Parsed::Unrecognized("RANDOM_REDIRECTX".to_string())
        );
    }

    /// 目标为空照样解析成空串（"这是错误"由上层判定，解析层不替它决定）
    #[test]
    fn redirect_without_target_parses_as_empty() {
        assert_eq!(parse_ok("$$COMMAND$$\nREDIRECT:\n").argument, "");
    }

    /// 界面标注：三种情况都要有短名与中文名
    #[test]
    fn kind_and_label_cover_every_case() {
        assert_eq!(parse("正文\n").kind(), None, "不是指令页面就没有标注");
        assert_eq!(parse("$$COMMAND$$\nREDIRECT: X\n").kind(), Some("redirect"));
        assert_eq!(
            parse("$$COMMAND$$\nRANDOM_REDIRECT\n").kind(),
            Some("random-redirect")
        );
        assert_eq!(parse("$$COMMAND$$\n").kind(), Some("unrecognized"));
        assert_eq!(parse("$$COMMAND$$\n$$COMMAND$$\n").kind(), Some("unrecognized"));

        for markdown in [
            "$$COMMAND$$\nREDIRECT: X\n",
            "$$COMMAND$$\nRANDOM_REDIRECT\n",
            "$$COMMAND$$\n",
            "$$COMMAND$$\n乱写\n",
        ] {
            let parsed = parse(markdown);
            assert!(parsed.label().is_some(), "{markdown:?} 应当有中文名");
            assert!(!parsed.describe().is_empty(), "{markdown:?} 应当有说明");
        }
    }

    /// **表要自洽**：短名与前缀都不许重复，说明与跳转都得写全 ——
    /// 这是"加新指令只改一处"的安全网，漏写立刻在这里失败。
    #[test]
    fn command_table_is_self_consistent() {
        for spec in COMMANDS {
            assert!(!spec.prefix.is_empty(), "{} 缺前缀", spec.kind);
            assert!(!spec.kind.is_empty(), "{} 缺短名", spec.prefix);
            assert!(!spec.label.is_empty(), "{} 缺中文名", spec.kind);
        }

        for (index, spec) in COMMANDS.iter().enumerate() {
            for other in &COMMANDS[index + 1..] {
                assert_ne!(spec.kind, other.kind, "短名重复");
                assert_ne!(spec.prefix, other.prefix, "前缀重复");
                // 前缀互为开头时，先写的会永远抢到匹配，必须避免
                assert!(
                    !spec.prefix.starts_with(other.prefix) && !other.prefix.starts_with(spec.prefix),
                    "前缀互为前缀：{} / {}",
                    spec.prefix,
                    other.prefix
                );
            }
        }

        assert!(supported().contains("REDIRECT"), "报错清单要由表生成");
    }
}
