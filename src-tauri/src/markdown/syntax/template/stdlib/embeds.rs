//! 把内容**原样写出去**的块：`code` / `mermaid` / `math` / `css` / `html` / `js`。
//!
//! 这一组的共同规矩是**正文一概不按 markdown 解析**，但理由分三种，各有各的讲究：
//!
//! - `code` / `mermaid` / `math`：内容是**另一种语言**（代码、图定义、TeX），里面的
//!   `*`、`_`、`\`、`-->`、`[]` 都是它自己的记号，按 markdown 解析就毁了。
//! - `css` / `js`：内容是**要执行的东西**，还要先过一遍 `{{参数}}` 填空与过滤
//!   （见 [`fill`]）——过滤与转义在那里，不在这里。
//! - `html`：内容是**要落进页面的标记**，默认过滤掉会执行脚本的东西，除非显式写了 `js`。
//!
//! 这一组都不碰 `node.children`：块正文走 `template.body`（原文）。

use super::super::dispatch::render_problem;
use super::super::fill;
use super::super::parse::Template;
use markdown_it::{Node, Renderer};

/// `::js` —— 把内容当**原始 JavaScript** 注入这一页，随页面加载执行。
///
/// 与 `::html` 一个规矩：内容原样，`{{参数}}` 会替换（见 [`fill`]）。不过滤 ——
/// 打开它（写下 `::js` 本身）就是一次显式的决定，与 `::html js` 是同一个意思。
///
/// 注入的脚本里若出现 `</script`，HTML 解析器会当场收尾，后面的字会漏到页面上，
/// 所以那一处先转义（见 [`fill::escape_script_end`]）。
///
/// 前端在正文注入 DOM 之后把 `<script>` 重新装成真节点（`v-html` 塞进去的脚本不会执行），
/// 所以这段代码才会真的跑起来。
pub(super) fn render_js(template: &Template, _node: &Node, fmt: &mut dyn Renderer) {
    let source = match fill::source_of(template) {
        Ok(source) => source,
        Err(why) => {
            render_problem(template, fmt, &why);
            return;
        }
    };
    let filled = fill::substitute(&source, template, &template.body);

    fmt.cr();
    fmt.open("script", &[("class", "note-js".to_string())]);
    fmt.text_raw(&fill::escape_script_end(&filled));
    fmt.close("script");
    fmt.cr();
}

/// `::mermaid` —— 画一张图（[mermaid](https://mermaid.js.org) 的语法）。
///
/// 这里只把**原文**放进 `<pre class="mermaid">`，画图在前端（mermaid 是 JS）。
/// 与 `::code` 同一个道理：图定义里的 `-->`、`[]`、`{}` 都是它自己的记号，
/// 绝不能按 markdown 解析。
///
/// 原文同时留一份在 `data-source` 上：mermaid 画完会把元素内容换成 SVG，
/// 切主题要重画时就靠这份原文还原（见前端 `dom/diagrams.ts`）。
pub(super) fn render_mermaid(template: &Template, _node: &Node, fmt: &mut dyn Renderer) {
    let source = template.body.trim();
    if source.is_empty() {
        render_problem(template, fmt, "里面还没有图 —— 写一段 mermaid 定义");
        return;
    }

    fmt.cr();
    fmt.open("div", &[("class", "diagram".to_string())]);
    fmt.cr();
    fmt.open(
        "pre",
        &[
            ("class", "mermaid".to_string()),
            ("data-source", source.to_string()),
        ],
    );
    fmt.text(source);
    fmt.close("pre");
    fmt.cr();
    fmt.close("div");
    fmt.cr();
}

/// `::math` —— 独立成段（或多行）的公式，TeX 原文，前端用 KaTeX 排版。
///
/// 正文**不按 markdown 解析**：公式里的 `*`、`_`、`\` 都是数学的一部分
/// （与 `::code`、`::mermaid` 同一条规矩）。
/// 只写一行的公式也可以用 `$…$` / `$$…$$`，那是 `syntax/math.rs` 认的。
pub(super) fn render_math(template: &Template, _node: &Node, fmt: &mut dyn Renderer) {
    let tex = template.body.trim();
    if tex.is_empty() {
        render_problem(template, fmt, "里面还没有公式 —— 写一段 TeX");
        return;
    }

    fmt.cr();
    fmt.open(
        "div",
        &[
            ("class", "math math--display".to_string()),
            ("data-tex", tex.to_string()),
        ],
    );
    // 排版之前看到的是原文，取不到 KaTeX 也不至于是一片空白
    fmt.text(tex);
    fmt.close("div");
    fmt.cr();
}

/// `::code lang=rust lines=off start=10 highlight=2-3` —— 像 markdown 的代码块，
/// 但能控制行号与要强调的行。
///
/// 这里**只输出代码原文与几个 `data-*` 提示**：行号列与强调色带由前端落地。
/// 为什么不在这里生成行号或拆行：拆行会破坏 highlight.js 的分词（它的 span 可能跨行），
/// 而"界面怎么显示"本来就属于前端。
pub(super) fn render_code(template: &Template, _node: &Node, fmt: &mut dyn Renderer) {
    let language = template.param("lang").unwrap_or("").trim();

    let mut attrs: Vec<(&str, String)> = vec![("class", "template-code".to_string())];
    if let Some(lines) = template.param("lines") {
        let off = matches!(lines.trim(), "off" | "false" | "no");
        attrs.push(("data-lines", if off { "off" } else { "on" }.to_string()));
    }
    if let Some(start) = template.param("start") {
        attrs.push(("data-line-start", start.trim().to_string()));
    }
    if let Some(highlight) = template.param("highlight") {
        attrs.push(("data-highlight", highlight.trim().to_string()));
    }

    let code_attrs: Vec<(&str, String)> = if language.is_empty() {
        Vec::new()
    } else {
        vec![("class", format!("language-{language}"))]
    };

    fmt.cr();
    fmt.open("pre", &attrs);
    fmt.open("code", &code_attrs);
    // 原文照收：代码里的 markdown 记号**不该**被解析
    fmt.text(&template.body);
    fmt.close("code");
    fmt.close("pre");
    fmt.cr();
}

/// `::css` —— 把内容（或 `src="页面名"` 指的模板页）当 CSS 注入页面。
///
/// 注入的 CSS 与界面在**同一个文档**里（作用域收在这一页的内容上），所以本项目所有的 CSS 变量
/// （`--accent`、`--accent-solid`、`--link-blue`、`--text-dim`、`--surface`…）
/// 在这里用 `var()` 直接就能取到 —— 这正是"自定义模板能跟着主题走"的关键。
///
/// 内容里的 `{{参数}}` 会被替换；`</style` 会被去掉，免得提前闭合样式块。
pub(super) fn render_css(template: &Template, _node: &Node, fmt: &mut dyn Renderer) {
    let source = match fill::source_of(template) {
        Ok(source) => source,
        Err(why) => {
            render_problem(template, fmt, &why);
            return;
        }
    };
    let css = fill::sanitize_css(&fill::substitute(&source, template, &template.body));
    // 收进这一页：不然一条 `* { }` 就能把整个界面改掉
    let css = fill::scope_css(&css);
    fmt.cr();
    fmt.open("style", &[]);
    fmt.text_raw(&css);
    fmt.close("style");
    fmt.cr();
}

/// `::html` —— 把内容当**原始 HTML** 注入。
///
/// 默认**过滤**掉会执行脚本的东西（`<script>`、`on*=` 事件属性、`javascript:` 协议）；
/// 只有显式写了 `js`（或 `js=true`）才原样放行。默认安全，要开就得自己写出来。
pub(super) fn render_html(template: &Template, _node: &Node, fmt: &mut dyn Renderer) {
    let source = match fill::source_of(template) {
        Ok(source) => source,
        Err(why) => {
            render_problem(template, fmt, &why);
            return;
        }
    };
    let filled = fill::substitute(&source, template, &template.body);
    let html = if fill::allows_js(template) {
        filled
    } else {
        fill::sanitize_html(&filled)
    };
    fmt.cr();
    fmt.text_raw(&html);
    fmt.cr();
}

#[cfg(test)]
mod tests {
    use crate::markdown::render;

    /// `::js`：内容原样进 <script>，只是 `</script` 会被转义掉
    #[test]
    fn js_goes_in_raw_and_cannot_close_its_own_tag() {
        let html = render("::js\n  console.log(\"你好\");\n");
        assert!(html.contains("<script"), "{html}");
        assert!(html.contains("console.log(\"你好\");"), "{html}");

        let tricky = render("::js\n  const s = \"</script>\";\n");
        assert!(
            !tricky.contains("</script>\";"),
            "不能让它提前闭合：{tricky}"
        );
        assert!(tricky.contains("<\\/script>"), "{tricky}");

        // 大小写照原样留着：那可能是字符串里的内容，改一个字母就改了它的意思
        let shouted = render("::js\n  const s = \"</SCRIPT>\";\n");
        assert!(shouted.contains("<\\/SCRIPT>"), "{shouted}");
    }

    #[test]
    fn css_template_injects_style_with_our_variables() {
        // 注入的 CSS 与界面同一个文档，所以 var(--accent) 这类变量直接用
        let html = render("::css\n  .我有自己的样式 { color: var(--accent); }\n");
        assert!(html.contains("<style>"), "{html}");
        assert!(html.contains("var(--accent)"), "{html}");
    }

    #[test]
    fn css_cannot_close_the_style_block_early() {
        let html = render("::css\n  a { } </style><p>顶出来了</p>\n");
        assert_eq!(html.matches("</style>").count(), 1, "{html}");
        assert!(
            html.contains("<p>顶出来了</p>"),
            "它仍然只是普通正文：{html}"
        );
    }

    #[test]
    fn html_template_filters_scripts_by_default() {
        let html = render("::html\n  <b>粗</b><script>alert(1)</script>\n");
        assert!(html.contains("<b>粗</b>"), "{html}");
        assert!(!html.contains("<script"), "默认必须过滤掉脚本：{html}");
    }

    #[test]
    fn html_template_allows_scripts_only_when_asked() {
        let html = render("::html js\n  <script>alert(1)</script>\n");
        assert!(html.contains("<script>alert(1)</script>"), "{html}");
    }

    #[test]
    fn placeholders_are_filled_from_params() {
        let html = render("::css 颜色=red\n  .a { color: {{颜色}}; }\n");
        assert!(html.contains("color: red"), "{html}");
    }

    #[test]
    fn code_template_carries_the_frontend_hints() {
        let html = render(
            "::code lang=rust lines=off start=10 highlight=2-3\n  fn main() {}\n  // 注释\n",
        );
        assert!(html.contains(r#"<pre class="template-code""#), "{html}");
        assert!(html.contains(r#"data-lines="off""#), "{html}");
        assert!(html.contains(r#"data-line-start="10""#), "{html}");
        assert!(html.contains(r#"data-highlight="2-3""#), "{html}");
        assert!(html.contains(r#"<code class="language-rust">"#), "{html}");
        assert!(html.contains("fn main() {}"), "{html}");
    }

    #[test]
    fn code_template_does_not_parse_markdown() {
        // 代码里的记号不该被当成 markdown（这是"像代码块"的关键）
        let html = render("::code\n  **不该变粗**\n");
        assert!(html.contains("**不该变粗**"), "{html}");
        assert!(!html.contains("<strong>"), "{html}");
    }

    /// `::mermaid`：原文照收（图定义里的记号不该被 markdown 解析），
    /// 另存一份在 `data-source` 上给"切主题重画"用
    #[test]
    fn mermaid_keeps_its_source_verbatim() {
        let html =
            render("::mermaid\n  graph TD\n    A[开始] --> B{行不行}\n     B -->|行| C[干活]\n");
        assert!(html.contains(r#"<pre class="mermaid""#), "{html}");
        assert!(
            html.contains("A[开始] --&gt; B{行不行}"),
            "箭头照原样：{html}"
        );
        assert!(html.contains("data-source="), "原文要留一份：{html}");
        // 图定义里的 `-->` `{}` 不该变成 HTML 标签
        assert!(!html.contains("<b>"), "{html}");
    }

    #[test]
    fn mermaid_without_content_says_so() {
        let html = render("::mermaid\n");
        assert!(html.contains("template--problem"), "{html}");
    }

    /// `::math`：TeX 原文进 `data-tex`，元素里留着原文当兜底
    #[test]
    fn math_template_hands_the_source_to_the_frontend() {
        let html = render("::math\n  \\frac{a}{b} = c_1 * d\n");
        assert!(html.contains("math--display"), "{html}");
        assert!(
            html.contains(r#"data-tex="\frac{a}{b} = c_1 * d""#),
            "{html}"
        );
        // `*` 与 `_` 是公式的一部分，不该被当成强调
        assert!(!html.contains("<em>"), "{html}");
    }
}
