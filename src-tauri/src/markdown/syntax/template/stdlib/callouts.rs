//! 提示框：`note` / `tip` / `warning` / `danger` / `error`。
//!
//! 五个共用一个渲染器，只有三样东西不同：**记号**、**配色那一档**、**默认标题**。
//! 于是它们长得像一家人，用起来也像一家人 —— 而"这是哪一种提示"这件事，读者是
//! 一眼扫记号认出来的，不是读文字认出来的。
//!
//! 与 [`super::text`] 的分界：那一组的模板（`quote` / `aside` / `banner` …）**形状**
//! 各不相同，所以各写各的；这一组形状完全一样，差别只是"哪一种"，所以共用一个
//! 渲染器 + 一张五行的小表。加第六种提示 = 加一行，不动别的。
//!
//! 内容**照常按 markdown 解析**（同 `text` 那一组）：提示框里当然要能写列表、链接、
//! 内部链接，也能再嵌一个 `::` 块。
//!
//! 配色只有一个 `color=` 参数能改（底色，字色由 [`super::text_on`] 按亮度定）——
//! 与 `::banner` 同一套规矩：`color=` 只认 `#rgb` / `#rrggbb`，因为这个值要写进
//! `style` 属性，宽松了就等于允许塞任意声明。

use super::super::dispatch::render_problem;
use super::super::parse::Template;
use super::{normalize_hex, text_on};
use markdown_it::{Node, Renderer};

/// 一种提示。三个字段就是这一族模板的全部差别。
struct Callout {
    /// 配色那一档：会写进 `class="callout callout--note"`
    class: &'static str,
    /// 标题前那个记号。**能被读屏软件读出来的标题文字另有一条**，所以它带
    /// `aria-hidden` —— 记号是给眼睛扫的，不是给耳朵读的。
    mark: &'static str,
    /// 没给 `title=` 时用的标题
    default_title: &'static str,
}

/// 五种提示。**按语气从轻到重排**，与文档里介绍它们的次序一致。
const CALLOUTS: [Callout; 5] = [
    Callout {
        class: "note",
        mark: "ℹ",
        default_title: "说明",
    },
    Callout {
        class: "tip",
        mark: "★",
        default_title: "建议",
    },
    Callout {
        class: "warning",
        mark: "⚠",
        default_title: "注意",
    },
    Callout {
        class: "danger",
        mark: "▲",
        default_title: "危险",
    },
    Callout {
        class: "error",
        mark: "✕",
        default_title: "错误",
    },
];

/// 按名字挑一种；表里没有就退到第一种（正常情况下不会走到 —— 调用方从
/// [`CALLOUTS`] 取对应的 `callout` 函数，走不到这条）。
fn pick(class: &str) -> &'static Callout {
    CALLOUTS
        .iter()
        .find(|one| one.class == class)
        .unwrap_or(&CALLOUTS[0])
}

/// 真正干活的那个。
fn render_one(template: &Template, node: &Node, fmt: &mut dyn Renderer, kind: &'static Callout) {
    // 空内容的提示框等于什么都没说 —— 与 `::banner` 一样，如实报错而不是渲染一个空框
    if node.children.is_empty() {
        render_problem(
            template,
            fmt,
            "内容是空的：正文至少写一行，或用 title= 给个标题再补上正文",
        );
        return;
    }

    let mut class = format!("callout callout--{}", kind.class);
    let mut style = String::new();
    // 与 `::banner` 同一套校验：这个值直接进 style 属性
    if let Some(color) = template.param("color") {
        let Some(hex) = normalize_hex(color) else {
            render_problem(template, fmt, "color= 只认 #rgb 或 #rrggbb 这两种写法");
            return;
        };
        style.push_str(&format!("background: {hex}; color: {};", text_on(&hex)));
        class.push_str(" callout--custom");
    }

    let title = template.param("title").unwrap_or(kind.default_title);

    let mut attrs: Vec<(&str, String)> = vec![("class", class)];
    if !style.is_empty() {
        attrs.push(("style", style));
    }

    fmt.cr();
    fmt.open("div", &attrs);
    fmt.cr();
    fmt.open("p", &[("class", "callout__title".to_string())]);
    // 记号单独包一层：样式要能把它摆正（字号、字距），也免得它跟标题文字连成一片
    fmt.open(
        "span",
        &[
            ("class", "callout__mark".to_string()),
            ("aria-hidden", "true".to_string()),
        ],
    );
    fmt.text(kind.mark);
    fmt.close("span");
    fmt.text(title);
    fmt.close("p");
    fmt.cr();
    fmt.contents(&node.children);
    fmt.cr();
    fmt.close("div");
    fmt.cr();
}

/// `::note` —— 说明。语气最轻：补充背景、交代一句来龙去脉。
pub(super) fn render_note(template: &Template, node: &Node, fmt: &mut dyn Renderer) {
    render_one(template, node, fmt, pick("note"));
}

/// `::tip` —— 建议。值得一试的做法，或者更省事的路子。
pub(super) fn render_tip(template: &Template, node: &Node, fmt: &mut dyn Renderer) {
    render_one(template, node, fmt, pick("tip"));
}

/// `::warning` —— 注意。不做会出麻烦，但眼下还没坏。
pub(super) fn render_warning(template: &Template, node: &Node, fmt: &mut dyn Renderer) {
    render_one(template, node, fmt, pick("warning"));
}

/// `::danger` —— 危险。会丢东西、会不可逆的那类操作。
pub(super) fn render_danger(template: &Template, node: &Node, fmt: &mut dyn Renderer) {
    render_one(template, node, fmt, pick("danger"));
}

/// `::error` —— 错误。已经出错了，或者这样写就是对不上的。
pub(super) fn render_error(template: &Template, node: &Node, fmt: &mut dyn Renderer) {
    render_one(template, node, fmt, pick("error"));
}

#[cfg(test)]
mod tests {
    use crate::markdown::render;

    /// 五种提示各出一份：记号、配色那一档、默认标题都要在
    #[test]
    fn each_callout_carries_its_own_mark_and_class() {
        for (name, class, mark, title) in [
            ("note", "note", "ℹ", "说明"),
            ("tip", "tip", "★", "建议"),
            ("warning", "warning", "⚠", "注意"),
            ("danger", "danger", "▲", "危险"),
            ("error", "error", "✕", "错误"),
        ] {
            let html = render(&format!("::{name}\n  正文一行\n"));
            assert!(
                html.contains(&format!("callout callout--{class}")),
                "{name}: {html}"
            );
            assert!(html.contains(mark), "{name} 的记号丢了: {html}");
            assert!(html.contains(title), "{name} 的默认标题丢了: {html}");
        }
    }

    /// 记号只是给眼睛扫的，读屏软件要能只读到标题文字
    #[test]
    fn the_mark_is_hidden_from_screen_readers() {
        let html = render("::note\n  正文\n");
        assert!(html.contains("aria-hidden=\"true\""), "{html}");
    }

    /// 提示框是**容器**，正文该按块渲染：可以有列表，不必把单段落摊平
    #[test]
    fn the_body_is_rendered_as_blocks() {
        let html = render("::note\n  第一段\n\n  - 甲\n  - 乙\n");
        assert!(html.contains("<li>甲</li>"), "{html}");
        assert!(html.contains("<p>第一段</p>"), "{html}");

        let bold = render("::tip\n  这里有**加粗**\n");
        assert!(bold.contains("<strong>加粗</strong>"), "{bold}");
    }

    /// `title=` 换掉默认标题，记号仍在
    #[test]
    fn a_title_parameter_replaces_the_default() {
        let html = render("::warning title=\"关于同步\"\n  正文\n");
        assert!(html.contains("关于同步"), "{html}");
        assert!(!html.contains("注意"), "默认标题该被换掉: {html}");
        assert!(html.contains("⚠"), "记号不该被换掉: {html}");
    }

    /// `color=` 只认十六进制：这个值直接写进 style 属性
    #[test]
    fn a_custom_color_is_validated_like_banner_does() {
        let colored = render("::note color=#0055a4\n  正文\n");
        assert!(colored.contains("background: #0055a4"), "{colored}");
        // 底色定了，字色由亮度决定，不让人自己写
        assert!(colored.contains("color: #f5f5f5"), "{colored}");
        assert!(colored.contains("callout--custom"), "{colored}");

        // 不合法就整条拒掉：给的是错误框，不是"照写但少个底色"
        let bad = render("::note color=\"red; position:fixed\"\n  正文\n");
        assert!(bad.contains("只认 #rgb 或 #rrggbb"), "{bad}");
        assert!(bad.contains("template--problem"), "{bad}");
        // 错误框会把原样参数回显出来（那是给人看出自己写了什么的），所以不能断言
        // 输出里没有那个字符串 —— 要断言的是没有 style 属性被放出去
        assert!(!bad.contains("style="), "{bad}");
    }

    /// 空内容的提示框如实报错，不渲染一个空框
    #[test]
    fn an_empty_callout_says_so() {
        let html = render("::danger\n");
        assert!(html.contains("内容是空的"), "{html}");
    }
}
