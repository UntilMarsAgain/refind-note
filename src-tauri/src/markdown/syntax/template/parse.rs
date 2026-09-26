//! 模板块的**头解析**：词法（引号、转义）与 `::名字 key=value …` 的拆解。
//!
//! 单独一个文件，是因为这里能写的东西太多：引号、转义、开关式参数，往后还可能有位置参数、
//! 参数值里的插值等等。块的边界识别在 `mod.rs`，渲染在 `dispatch.rs` 与 `stdlib.rs`。

/// 一个模板块：头（名字 + 参数）、块内容，以及**解析好的内容**。
///
/// 内容交给整个解析器再解析一遍，子节点挂在节点的 `children` 上（与 blockquote 等
/// 容器块同一套做法），渲染器只管把它们写出去。
#[derive(Debug, Clone, PartialEq)]
pub struct Template {
    /// `::` 后面的名字（分发就用它）
    pub name: String,
    /// `key=value` 参数；只写了 `key` 的开关式参数，值是空串
    pub params: Vec<(String, String)>,
    /// 块内容原文，已去掉公共缩进（未知模板要把它原样显示出来）
    pub body: String,
}

impl Template {
    /// 解析头一行：`::名字 key=value …`。不是头就返回 `None`。
    ///
    /// 名字必须紧跟在 `::` 之后，且不能含 `=` / `:` —— 那两个字符属于参数与命名空间，
    /// 出现在名字里几乎一定是写错了。宁可退回普通段落，也不要认出一个坏块。
    pub fn parse_header(line: &str) -> Option<(String, Vec<(String, String)>)> {
        let rest = line.trim_start().strip_prefix("::")?;
        let mut tokens = tokenize(rest)?.into_iter();
        let name = tokens.next()?;
        if name.is_empty() || name.contains('=') || name.contains(':') {
            return None;
        }
        // `=` 按**第一个**切：`key="a=b"` 的值就是 `a=b`。
        let params = tokens
            .map(|token| match token.split_once('=') {
                Some((key, value)) => (key.to_string(), value.to_string()),
                None => (token.to_string(), String::new()),
            })
            .collect();
        Some((name, params))
    }

    /// 取一个参数（渲染器要用）
    pub fn param(&self, key: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
    }
}

/// 把一行切成 token：空白分隔，但**引号里的空白不算分隔**。
///
/// 单引号与双引号都可以；引号内用反斜杠保护引号本身与反斜杠，引号外反斜杠没有特殊含义
/// （路径里到处都是反斜杠，不该在这里被吃掉）。
///
/// 引号没有配对就整行走不通：返回 `None`，这一行退回普通段落 —— 宁可当普通文字，
/// 也不要猜作者想要什么。
pub fn tokenize(line: &str) -> Option<Vec<String>> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut started = false;
    let mut quote: Option<char> = None;
    let mut chars = line.chars();

    while let Some(ch) = chars.next() {
        match quote {
            Some(closing) => {
                if ch == '\\' {
                    match chars.next() {
                        Some('\\') => current.push('\\'),
                        Some(next) if next == closing => current.push(closing),
                        // 只认这两个转义，其余原样保留（连反斜杠一起）
                        Some(next) => {
                            current.push('\\');
                            current.push(next);
                        }
                        None => return None,
                    }
                } else if ch == closing {
                    quote = None;
                } else {
                    current.push(ch);
                }
            }
            None => match ch {
                '\'' | '"' => {
                    quote = Some(ch);
                    // `""` 是一个空值的 token，不是没有 token
                    started = true;
                }
                ch if ch.is_whitespace() => {
                    if started {
                        tokens.push(std::mem::take(&mut current));
                        started = false;
                    }
                }
                ch => {
                    current.push(ch);
                    started = true;
                }
            },
        }
    }

    if quote.is_some() {
        return None;
    }
    if started {
        tokens.push(current);
    }
    Some(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_name_and_params() {
        let (name, params) = Template::parse_header("::note key=value flag 另一个=值").unwrap();
        assert_eq!(name, "note");
        assert_eq!(
            params,
            vec![
                ("key".to_string(), "value".to_string()),
                // 只写 key 的开关式参数也认
                ("flag".to_string(), String::new()),
                ("另一个".to_string(), "值".to_string()),
            ]
        );
    }

    #[test]
    fn quotes_protect_spaces_and_escapes() {
        let (name, params) =
            Template::parse_header(r#"::"两 个词" 标题="含 空格" 引号="他说\"好\"" flag"#).unwrap();
        assert_eq!(name, "两 个词");
        assert_eq!(
            params,
            vec![
                ("标题".to_string(), "含 空格".to_string()),
                ("引号".to_string(), "他说\"好\"".to_string()),
                ("flag".to_string(), String::new()),
            ]
        );
    }

    #[test]
    fn rejects_lines_that_are_not_headers() {
        assert!(Template::parse_header("::").is_none());
        assert!(Template::parse_header("::=x").is_none());
        assert!(Template::parse_header("::a:b").is_none());
        assert!(Template::parse_header(":name").is_none());
        assert!(Template::parse_header("普通一行").is_none());
    }

    #[test]
    fn unterminated_quote_is_not_a_header() {
        assert!(Template::parse_header("::note key=\"没关上").is_none());
    }

    #[test]
    fn backslash_outside_quotes_is_literal() {
        let (_, params) = Template::parse_header(r"::note path=C:\笔记").unwrap();
        assert_eq!(params, vec![("path".to_string(), r"C:\笔记".to_string())]);
    }
}
