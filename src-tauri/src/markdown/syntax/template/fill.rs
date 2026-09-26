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

/// 把用户写的 CSS 收进正文范围：每条选择器都加 `.note-body` 前缀。
///
/// 为什么不原样注入：那样它作用于**整个界面** —— 一条 `* { }` 就能把标签栏、菜单、
/// 任务栏一起改掉。收进正文之后，它只影响正文；而正文内部仍按正常层叠参与，
/// 注入的样式在文档里靠后，同优先级的规则由它胜出。
///
/// 顺带处理两件事：`@import` 一律丢掉（会让笔记去外部拉样式），
/// `@media` 之类的条件规则递归处理（里面的选择器同样要收）。
///
/// 不是完整的 CSS 解析器：花括号不配对时原样放行，字符串里的花括号也不特殊处理 ——
/// CSS 里这两种情况都很罕见，而真遇到时"原样放行"比"猜错"好。
pub fn scope_css(css: &str) -> String {
    scope_css_raw(css).trim().to_string()
}

/// 不做两端裁剪的版本：递归处理条件规则时要保留里面的空白
fn scope_css_raw(css: &str) -> String {
    let css = drop_imports(css);
    let mut out = String::with_capacity(css.len() + 16);
    let mut rest = css.as_str();

    while let Some(position) = rest.find('{') {
        let head = &rest[..position];
        let after = &rest[position..];

        // 找配对的 `}`
        let mut depth = 0usize;
        let mut close = None;
        for (index, ch) in after.char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        close = Some(index);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(close) = close else {
            out.push_str(rest);
            return out;
        };

        let body = &after[1..close];
        rest = &after[close + 1..];
        let trimmed = head.trim();

        if trimmed.starts_with('@') {
            let keyword = trimmed
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_lowercase();
            match keyword.as_str() {
                // 条件规则：里面的选择器同样要收
                "@media" | "@supports" | "@layer" | "@container" | "@scope" => {
                    out.push_str(head);
                    out.push('{');
                    out.push_str(&scope_css_raw(body));
                    out.push('}');
                }
                // 关键帧、字体、分页：名字是全局的，选择器不是，原样保留
                _ => {
                    out.push_str(head);
                    out.push('{');
                    out.push_str(body);
                    out.push('}');
                }
            }
            continue;
        }

        // 保留选择器前后的空白（缩进、以及 `{` 前那个空格），中间那截换成收窄后的选择器
        let lead = &head[..head.len() - head.trim_start().len()];
        let trail = &head[head.trim_end().len()..];
        out.push_str(lead);
        let scoped: Vec<String> = split_selectors(trimmed)
            .iter()
            .map(|selector| scope_selector(selector))
            .filter(|selector| !selector.is_empty())
            .collect();
        out.push_str(&scoped.join(", "));
        out.push_str(trail);
        out.push('{');
        out.push_str(body);
        out.push('}');
    }
    out.push_str(rest);
    out
}

/// 丢掉 `@import …;`（大小写不敏感）
fn drop_imports(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(position) = find_ci(rest, "@import", 0) {
        out.push_str(&rest[..position]);
        let tail = &rest[position..];
        match tail.find(';') {
            Some(end) => rest = &tail[end + 1..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

/// 按**顶层**逗号切开选择器（`:is(a, b)` 里的逗号不算）
fn split_selectors(selector: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut depth = 0i32;
    for ch in selector.chars() {
        match ch {
            '(' | '[' => {
                depth += 1;
                current.push(ch);
            }
            ')' | ']' => {
                depth -= 1;
                current.push(ch);
            }
            ',' if depth == 0 => parts.push(std::mem::take(&mut current)),
            _ => current.push(ch),
        }
    }
    parts.push(current);
    parts
}

/// 给一条选择器加范围。
fn scope_selector(selector: &str) -> String {
    let selector = selector.trim();
    if selector.is_empty() || selector.starts_with(".note-body") || selector.starts_with('&') {
        return selector.to_string();
    }
    // 写给根元素的规则收成正文本身：于是"给整篇正文设字体或变量"仍然写得出来
    for root in ["html", "body", ":root"] {
        if selector == root {
            return ".note-body".to_string();
        }
        if let Some(rest) = selector.strip_prefix(root) {
            if rest.starts_with('.') || rest.starts_with(':') || rest.starts_with('[') {
                return format!(".note-body{rest}");
            }
        }
    }
    format!(".note-body {selector}")
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
    fn css_is_scoped_to_the_note_body() {
        assert_eq!(scope_css(".a { color: red }"), ".note-body .a { color: red }");
        assert_eq!(
            scope_css(".a, .b { }"),
            ".note-body .a, .note-body .b { }"
        );
        // `:is(a, b)` 里的逗号不是选择器分隔符
        assert_eq!(scope_css(":is(a, b) { }"), ".note-body :is(a, b) { }");
        // 写给根元素的收成正文本身
        assert_eq!(scope_css("body { }"), ".note-body { }");
        assert_eq!(scope_css(":root { --x: 1 }"), ".note-body { --x: 1 }");
        // 已经收过的不重复收
        assert_eq!(scope_css(".note-body a { }"), ".note-body a { }");
    }

    #[test]
    fn conditional_rules_are_scoped_inside() {
        assert_eq!(
            scope_css("@media (min-width: 1px) { .a { } }"),
            "@media (min-width: 1px) { .note-body .a { } }"
        );
        // 关键帧里的百分比不是选择器，别去动它
        assert_eq!(
            scope_css("@keyframes spin { from { } to { } }"),
            "@keyframes spin { from { } to { } }"
        );
    }

    #[test]
    fn imports_are_dropped() {
        // 让笔记去外部拉样式：既慢又不受控
        assert_eq!(scope_css("@import url(x.css); .a { }"), ".note-body .a { }");
    }

    #[test]
    fn style_cannot_be_closed_early() {
        let css = sanitize_css("a { } </style><b>x</b>");
        assert!(!css.contains("</style"), "{css}");
        assert!(css.contains("<b>x</b>"), "只去掉闭合标签，其余照旧：{css}");
    }
}
