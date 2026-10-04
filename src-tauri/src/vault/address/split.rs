//! 把一个地址字符串切成三段：`命名空间` / `页面名称` / `浏览状态#章节`。
//!
//! **只切不问** —— 这一层不判断切出来的东西合不合法，那是 [`super::parse`] 与
//! [`crate::vault::title`] 的事。这样切分的规矩（哪几个符号是分隔符、`#` 之后
//! 原样保留）能单独讲清楚，不必混在一大段解析里。

// 字段也得是 `pub(super)`：子模块的**私有**方法父模块调得到，私有**字段**却调不到
// —— 父模块 `parse` 要拿这三段拼地址去。私有性是"逐层"判的，这一条容易踩。
pub(super) struct AddressParts {
    pub(super) name: String,
    pub(super) state: Option<String>,
    pub(super) section: Option<String>,
}

/// 把 `名称[@状态][#段落]` 拆成三部分。
///
/// 名称之外的成分各以 `@` / `#` 开头，所以**书写顺序自由**（`名称#段落@状态` 也认得）；
/// 同名成分后者覆盖前者（`a#x#y` 的段落是 `y`），空成分算没写。
pub(super) fn split_address(raw: &str) -> AddressParts {
    let name_end = raw
        .char_indices()
        .find(|(_, ch)| matches!(ch, '@' | '#'))
        .map(|(index, _)| index)
        .unwrap_or(raw.len());
    let name = raw[..name_end].trim().to_string();
    let rest = &raw[name_end..];

    let mut state: Option<String> = None;
    let mut section: Option<String> = None;
    let mut cursor = 0;
    while cursor < rest.len() {
        let marker = rest[cursor..].chars().next().expect("非空");
        let from = cursor + marker.len_utf8();
        let to = rest[from..]
            .find(['@', '#'])
            .map(|offset| from + offset)
            .unwrap_or(rest.len());
        let value = rest[from..to].trim().to_string();

        match marker {
            '@' => state = Some(value),
            '#' => section = Some(value),
            _ => {}
        }
        cursor = to;
    }

    AddressParts {
        name,
        state: state.filter(|value| !value.is_empty()),
        section: section.filter(|value| !value.is_empty()),
    }
}
