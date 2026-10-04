//   Refind Note is a note-taking software.
//   Copyright (C) 2026 Until Mars Again
//
//   This program is free software: you can redistribute it and/or modify
//   it under the terms of the GNU Affero General Public License as published by
//   the Free Software Foundation, either version 3 of the License, or
//   (at your option) any later version.
//
//   This program is distributed in the hope that it will be useful,
//   but WITHOUT ANY WARRANTY; without even the implied warranty of
//   MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
//   GNU Affero General Public License for more details.
//
//   You should have received a copy of the GNU Affero General Public License
//   along with this program.  If not, see <http://www.gnu.org/licenses/>.

//! 按名字分发：查标准模板表，查不到就渲染一个报错框。
use super::parse::Template;
use super::stdlib::TEMPLATES;
use markdown_it::{Node, Renderer};

/// 一个模板渲染器：拿到头、拿到**解析好的内容节点**，往 `fmt` 里写 HTML。
///
/// 之所以把 `node` 一起给出去：内容已经由整个解析器解析成子节点，渲染器直接
/// `fmt.contents(&node.children)` 就能用 —— 引用块、提示框这类容器模板正需要它。
pub type TemplateRenderer = fn(&Template, &Node, &mut dyn Renderer);

/// 分发：标准模板表 → 报错框。
pub fn render(template: &Template, node: &Node, fmt: &mut dyn Renderer) {
    if let Some((_, renderer)) = TEMPLATES.iter().find(|(name, _)| *name == template.name) {
        renderer(template, node, fmt);
        return;
    }
    render_unknown(template, fmt);
}

/// `problem` —— 说清**出了什么事**，并且**给得出下一步**。
///
/// 与 [`render_decrypt`] 分开是因为**原因不同**：`problem` 是"这一处用法有问题"
/// （比如模板套模板超过层数上限），`decrypt` 是"东西没问题，只是它上了锁"。
/// 给上锁的场合摆解锁框是对的；给用法错误摆一个输入框是荒唐的 —— 人输了口令，
/// 问题还在。
///
/// 不回显这次的参数：出问题的不是这次用法，回显出来只会把作者引到错的地方。
pub(super) fn render_problem_page(template: &Template, node: &Node, fmt: &mut dyn Renderer) {
    fmt.cr();
    fmt.open(
        "div",
        &[("class", "template template--problem".to_string())],
    );
    fmt.cr();
    fmt.open("p", &[("class", "template__head".to_string())]);
    fmt.open("span", &[("class", "template__badge".to_string())]);
    fmt.text("模板页读不出来");
    fmt.close("span");
    fmt.close("p");
    fmt.cr();
    fmt.open("p", &[("class", "template__why".to_string())]);
    fmt.text(&template.body);
    fmt.close("p");
    fmt.cr();
    fmt.open("p", &[("class", "template__where".to_string())]);
    fmt.contents(&node.children);
    fmt.close("p");
    fmt.cr();
    fmt.close("div");
    fmt.cr();
}

/// `decrypt` —— **读不出来的那一份**摆在原位上的解锁框。
///
/// ## 为什么它必须在注册表里
///
/// [`crate::markdown::syntax::template::expand`] 在模板页读不出来时拿不到节点，
/// 就手工拼一个 `name: "decrypt"` 的 `Template` 交给分发，而分发只认注册表。
/// 它不在表里的话，"这一页没解锁"会被显示成 `未知模板 :: decrypt` —— 作者看到的是
/// 自己写错了模板名，而真正的原因是**模板页上了锁**，两边对不上，这正是最难查的一类。
///
/// ## 它只出标记，不出输入框
///
/// 这里写出来的 `span[data-decrypt]` 只带**数据属性**（哪一种、哪一份、要不要口令、
/// 为什么读不出来）；输入框、按钮、"显示"这三个动作由前端 `dom/decrypt.ts` 补上。
///
/// 这么分是因为**同样的框有两条来路**，而信息到达的时间不一样：模板页是渲染时就知道，
/// 附件是前端异步探明之后才知道。两边都走同一个前端构造器（`decryptBox`），
/// 于是框的样子与行为只有**一处**定义 —— 标记只是把"这是个解锁框"这件事告诉前端。
///
/// 与 [`render_problem`] 分开是因为**原因不同**：一个是"名字认识、用法不对"，
/// 一个是"名字没问题，但它指的那一页读不出来"。所以这里**不回显这次的参数** ——
/// 出问题的不是这次用法，回显出来只会把作者引到错的地方。
///
/// 但**那一页本身的链接要给**（`expand::decrypt_node` 挂在 children 上的那个）：
/// 口令打不开时，能做的下一步仍然是去把那一页改掉，链接正是那一步。
pub(super) fn render_decrypt(template: &Template, node: &Node, fmt: &mut dyn Renderer) {
    fmt.cr();
    fmt.open(
        "div",
        &[("class", "template template--decrypt".to_string())],
    );
    fmt.cr();
    fmt.open("p", &[("class", "template__head".to_string())]);
    fmt.open("span", &[("class", "template__badge".to_string())]);
    fmt.text("模板页读不出来");
    fmt.close("span");
    fmt.close("p");
    fmt.cr();
    // `expand::decrypt_node` 把标题与原因放在这两个参数里
    fmt.open(
        "span",
        &[
            ("class", "decrypt".to_string()),
            ("data-decrypt", "yes".to_string()),
            ("data-decrypt-kind", "page".to_string()),
            (
                "data-decrypt-title",
                template.param("target").unwrap_or("").to_string(),
            ),
            (
                "data-decrypt-label",
                template.param("label").unwrap_or("").to_string(),
            ),
            ("data-decrypt-reason", template.body.to_string()),
            // 要不要口令输入框：**后端此刻已经知道**（`expand` 从封装头读来的），
            // 不写进来就得让前端再猜一次 —— 而猜错是静默的
            (
                "data-decrypt-needs-passphrase",
                match template.param("passphrase") {
                    Some("yes") => "yes",
                    _ => "no",
                }
                .to_string(),
            ),
        ],
    );
    fmt.close("span");
    fmt.cr();
    // **下一步**：指向那个模板页的链接（见上面为什么必须有它）
    fmt.open("p", &[("class", "template__where".to_string())]);
    fmt.contents(&node.children);
    fmt.close("p");
    fmt.cr();
    fmt.close("div");
    fmt.cr();
}

/// 名字查不到时的兜底：渲染一个框，把名字与参数原样列出，内容仍用代码块裹住。
///
/// 这是**给作者看的错误提示**，不是"不认识就静静丢掉"。参数回显成等号写法，
/// 所以从输出上就能看出解析成了什么 —— 引号去了哪儿、空格是否保住。
pub(super) fn render_unknown(template: &Template, fmt: &mut dyn Renderer) {
    fmt.cr();
    fmt.open(
        "div",
        &[("class", "template template--unknown".to_string())],
    );
    fmt.cr();
    head(fmt, "未知模板", template);
    fmt.cr();
    body_as_code(template, fmt);
    fmt.cr();
    fmt.close("div");
    fmt.cr();
}

/// 名字认识、但用法不对时的提示。
///
/// 与"未知模板"分开：一个是"没这个名字"，一个是"名字对、参数不对" ——
/// 提示词要跟着这个区别走，否则作者会往错的方向查。
pub(super) fn render_problem(template: &Template, fmt: &mut dyn Renderer, why: &str) {
    fmt.cr();
    fmt.open(
        "div",
        &[("class", "template template--problem".to_string())],
    );
    fmt.cr();
    head(fmt, "模板用法有问题", template);
    fmt.cr();
    fmt.open("p", &[("class", "template__why".to_string())]);
    fmt.text(why);
    fmt.close("p");
    fmt.cr();
    body_as_code(template, fmt);
    fmt.cr();
    fmt.close("div");
    fmt.cr();
}

/// 徽标 + 模板名 + 解析到的参数（回显成等号写法，一眼看出解析成了什么）
fn head(fmt: &mut dyn Renderer, badge: &str, template: &Template) {
    fmt.open("p", &[("class", "template__head".to_string())]);
    fmt.open("span", &[("class", "template__badge".to_string())]);
    fmt.text(badge);
    fmt.close("span");
    fmt.open("code", &[("class", "template__name".to_string())]);
    fmt.text(&template.name);
    fmt.close("code");
    for (key, value) in &template.params {
        fmt.open("code", &[("class", "template__param".to_string())]);
        let shown = if value.is_empty() {
            key.clone()
        } else {
            format!("{key}={value}")
        };
        fmt.text(&shown);
        fmt.close("code");
    }
    fmt.close("p");
}

/// 内容原样给出来（代码块）：写错了也不该让内容静静消失
fn body_as_code(template: &Template, fmt: &mut dyn Renderer) {
    fmt.open("pre", &[]);
    fmt.open("code", &[]);
    fmt.text(&template.body);
    fmt.close("code");
    fmt.close("pre");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 分节表里的名字都得是真模板：两张表一旦对不上，写下的名字会被当成"未知模板"，
    /// 而"要不要分节"却已经按这张表决定了 —— 这种半截状态最难查
    #[test]
    fn every_sectioned_name_is_a_real_template() {
        for name in super::super::stdlib::SECTIONED {
            assert!(
                TEMPLATES.iter().any(|(template, _)| template == name),
                "{name} 在分节表里，却不在模板表里"
            );
        }
    }

    /// `expand::text_node` 造的是 `name: "problem"` 的节点，而分发只认注册表。
    /// 它不在表里的话，"模板页没解锁"会被显示成"未知模板 :: problem" ——
    /// 作者看到的是自己写错了模板名，真正原因（那一页上了锁）一个字都不提。
    #[test]
    fn the_problem_template_is_really_registered() {
        assert!(
            TEMPLATES.iter().any(|(name, _)| *name == "problem"),
            "problem 不在模板表里，模板页读不出来会被显示成未知模板"
        );
    }

    #[test]
    fn dispatch_table_has_unique_names() {
        let mut names: Vec<&str> = TEMPLATES.iter().map(|(name, _)| *name).collect();
        let total = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(total, names.len(), "模板表里有重名");
    }
}
