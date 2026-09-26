//! 模板的"填空"与过滤。
//!
//! 两件事各管一半：`{{}}` 把参数与块内容填进模板；过滤决定放行的 HTML 能做什么。
//! 单独一个文件，是因为这里同样"能写的东西很多"（以后可能加条件、循环、默认值）。
use super::parse::Template;
use crate::markdown::current_resolver;

/// 把 `{{名字}}` 换成参数值，`{{body}}` 换成块内容。
///
/// 认不出的名字**原样留着** —— 让作者看见自己写错了，比悄悄换成空串好。
/// 没有闭合的 `{{` 也原样留着。
pub fn substitute(text: &str, template: &Template, body: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            out.push_str(&rest[start..]);
            return out;
        };
        let key = after[..end].trim();
        if key == "body" {
            out.push_str(body);
        } else if let Some(value) = template.param(key) {
            out.push_str(value);
        } else {
            out.push_str("{{");
            out.push_str(&after[..end]);
            out.push_str("}}");
        }
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    out
}

/// 模板的正文：`src="页面名"` 取模板命名空间里的页面，否则用块自身的内容。
///
/// 取不到（模板不存在、或不在渲染上下文里）返回 `None`。
pub fn source_of(template: &Template) -> Option<String> {
    match template.param("src") {
        Some(name) => current_resolver().and_then(|resolver| resolver.template(name).map(str::to_string)),
        None => Some(template.body.clone()),
    }
}

/// `js` 参数是否显式打开了（`js` 或 `js=true`）
pub fn allows_js(template: &Template) -> bool {
    template
        .params
        .iter()
        .any(|(key, value)| key == "js" && (value.is_empty() || value == "true"))
}

/// 去掉会执行脚本的东西：`<script>` 整块、`on*=` 事件属性、`javascript:` 协议。
///
/// **这不是一个完整的 HTML 净化器** —— 它只挡住"打开就会跑代码"这条线，够用于
/// "默认安全、要开 JS 得自己写出来"这个约定。真正的净化器要按白名单解析整个 HTML，
/// 那是另一个量级的东西，等真有需要再说。
pub fn sanitize_html(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;

    // 1) `<script …>…</script>` 整块拿掉；只剩开标签的也拿掉
    while let Some(start) = find_tag(rest, "script") {
        out.push_str(&rest[..start]);
        let after_open = &rest[start..];
        let close = find_ci(after_open, "</script", 0);
        match close {
            Some(position) => {
                let tail = &after_open[position..];
                let end = find_ci(tail, ">", 0).map(|i| position + i + 1).unwrap_or(position);
                rest = &after_open[end..];
            }
            None => {
                rest = "";
            }
        }
    }
    out.push_str(rest);
    let html = out;

    // 2) `on*=` 事件属性。只在**标签内部**认它 ——
    //    正文里写 `one=1`、`onboarding=2` 不该被当成事件属性拿掉。
    let mut out = String::with_capacity(html.len());
    let bytes: Vec<char> = html.chars().collect();
    let mut index = 0usize;
    let mut in_tag = false;
    while index < bytes.len() {
        if bytes[index] == '<' {
            in_tag = true;
            out.push('<');
            index += 1;
            continue;
        }
        if bytes[index] == '>' {
            in_tag = false;
            out.push('>');
            index += 1;
            continue;
        }
        if in_tag && is_on_attribute(&bytes, index) {
            // 跳到值结束：引号包住就跳到配对的引号，否则跳到空白或 `>`
            let mut cursor = index;
            while cursor < bytes.len() && bytes[cursor] != '=' {
                cursor += 1;
            }
            cursor += 1;
            while cursor < bytes.len() && bytes[cursor].is_whitespace() {
                cursor += 1;
            }
            if cursor < bytes.len() && (bytes[cursor] == '"' || bytes[cursor] == '\'') {
                let quote = bytes[cursor];
                cursor += 1;
                while cursor < bytes.len() && bytes[cursor] != quote {
                    cursor += 1;
                }
                cursor += 1;
            } else {
                while cursor < bytes.len() && !bytes[cursor].is_whitespace() && bytes[cursor] != '>' {
                    cursor += 1;
                }
            }
            index = cursor;
            continue;
        }
        out.push(bytes[index]);
        index += 1;
    }

    // 3) `javascript:` 协议
    strip_protocol(&out, "javascript:")
}

/// CSS 里不许出现 `</style`：它会提前闭合样式块，把后面的内容顶到页面上
pub fn sanitize_css(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(position) = find_ci(rest, "</style", 0) {
        out.push_str(&rest[..position]);
        // 连它后面到 `>` 的部分一起去掉
        let tail = &rest[position..];
        let end = find_ci(tail, ">", 0).map(|i| i + 1).unwrap_or(tail.len());
        rest = &tail[end..];
    }
    out.push_str(rest);
    out
}

/// 找 `<tag`（大小写不敏感，且后面不是字母，免得把 `<scripted` 当成 `<script`）
fn find_tag(haystack: &str, tag: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(position) = find_ci(haystack, &format!("<{tag}"), from) {
        let after = haystack[position + 1 + tag.len()..].chars().next();
        let boundary = match after {
            None => true,
            Some(ch) => !ch.is_alphanumeric() && ch != '-',
        };
        if boundary {
            return Some(position);
        }
        from = position + 1;
    }
    None
}

fn is_on_attribute(chars: &[char], index: usize) -> bool {
    if chars[index] != 'o' && chars[index] != 'O' {
        return false;
    }
    if index > 0 && (chars[index - 1].is_alphanumeric() || chars[index - 1] == '-') {
        return false;
    }
    let mut cursor = index + 1;
    if cursor >= chars.len() || (chars[cursor] != 'n' && chars[cursor] != 'N') {
        return false;
    }
    cursor += 1;
    let mut letters = 0;
    while cursor < chars.len() && (chars[cursor].is_alphanumeric() || chars[cursor] == '-') {
        cursor += 1;
        letters += 1;
    }
    if letters == 0 {
        return false;
    }
    while cursor < chars.len() && chars[cursor].is_whitespace() {
        cursor += 1;
    }
    cursor < chars.len() && chars[cursor] == '='
}

/// 大小写不敏感地找子串（只用于 ASCII 标签名与协议名）
fn find_ci(haystack: &str, needle: &str, from: usize) -> Option<usize> {
    if from > haystack.len() {
        return None;
    }
    let hay = haystack[from..].to_lowercase();
    let needle = needle.to_lowercase();
    hay.find(&needle).map(|position| position + from)
}

/// 去掉某个协议（大小写不敏感）
fn strip_protocol(html: &str, protocol: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(position) = find_ci(rest, protocol, 0) {
        out.push_str(&rest[..position]);
        rest = &rest[position + protocol.len()..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::syntax::template::parse::Template;

    fn template(name: &str, params: &[(&str, &str)], body: &str) -> Template {
        Template {
            name: name.to_string(),
            params: params
                .iter()
                .map(|(key, value)| (key.to_string(), value.to_string()))
                .collect(),
            body: body.to_string(),
        }
    }

    #[test]
    fn substitutes_params_and_body() {
        let item = template("x", &[("颜色", "红")], "块内容");
        assert_eq!(
            substitute("a {{颜色}} b {{body}} c", &item, "块内容"),
            "a 红 b 块内容 c"
        );
    }

    #[test]
    fn unknown_or_unclosed_placeholders_stay_visible() {
        let item = template("x", &[], "");
        assert_eq!(substitute("{{没有这个}}", &item, ""), "{{没有这个}}");
        assert_eq!(substitute("{{没闭合", &item, ""), "{{没闭合");
    }

    #[test]
    fn scripts_are_filtered_by_default() {
        let html = sanitize_html("<p>正文</p><script>alert(1)</script>");
        assert!(!html.contains("<script"), "{html}");
        assert!(html.contains("正文"), "{html}");
    }

    #[test]
    fn event_handlers_and_javascript_urls_are_filtered() {
        let html = sanitize_html(r#"<img src="x" onerror="alert(1)"><a href="javascript:alert(1)">x</a>"#);
        assert!(!html.contains("onerror"), "{html}");
        assert!(!html.contains("javascript:"), "{html}");
        // 无辜的属性与内容不受影响
        assert!(html.contains(r#"src="x""#), "{html}");
    }

    #[test]
    fn on_inside_ordinary_words_is_left_alone() {
        // 只有"属性位置上的 on…="才该被拿掉
        let html = sanitize_html("<p>one=1 与 onboarding=2</p>");
        assert!(html.contains("one=1"), "{html}");
        assert!(html.contains("onboarding=2"), "{html}");
    }

    #[test]
    fn style_cannot_be_closed_early() {
        let css = sanitize_css("a { } </style><b>x</b>");
        assert!(!css.contains("</style"), "{css}");
        assert!(css.contains("<b>x</b>"), "只去掉闭合标签，其余照旧：{css}");
    }
}
