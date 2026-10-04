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

/// `decrypt` —— 页内解锁。
///
/// ## 它是个**能给人写**的模板
///
/// ```text
/// ::decrypt page=卡片
///   卡片解开之后，这里替换它的位置。
/// ```
///
/// `page` 必填（要解锁的是哪一页）。行为只有两种：
///
/// - **那一页现在读得动** → 渲染底部内容（它是一个 markdown 子页面，
///   和别的模板的块内容一样重新过一遍解析器）；底部为空就摆一句"已解锁"。
/// - **读不动** → 摆解锁框（`span[data-decrypt]` 标记 + 几个数据属性）。
///   解开了以后调用方**重渲染这一篇**，`::decrypt` 自然就换成底部内容了。
///
/// 解析器发现某个模板页读不出来时，**就当作写了一个 `::decrypt page=那一页`**
/// （见 `expand::expand_user_template`）。所以那件事与作者亲手写的走的是同一条路。
///
/// ## `page=` 在哪个命名空间里找
///
/// 与 `::名字` **同一张表**（`Template:` 命名空间，见 `markdown::template_page`）。
/// 刻意不把普通笔记也塞进那张表：`template_page_key` 会把 `甲` 与 `Template:甲`
/// 归成同一个键，所以一旦把某篇笔记按页名放进去，`::那个名字` 也会跟着能用 ——
/// 那是悄悄改了别的模板的语义。代价是 `page=` 只能指模板页；这一点写在帮助页里。
///
/// ## 为什么这里有一道自己的深度限制
///
/// 底部内容是 `scanner` 用 `state.md.parse` 解析的（见 `scanner.rs` 第 113 行），
/// **没有**走 `markdown::deeper` —— 而 `MAX_TEMPLATE_DEPTH` 只在
/// `expand_user_template`（用户模板）里查。于是 `::decrypt` 是内置模板，
/// 自己套自己**不受那道限制**，会一路递归到爆栈。
///
/// 所以这里自己记一笔：`deeper_levels_stay_inside` 那种全局计数在这一支是瞎的。
pub(super) fn render_decrypt(template: &Template, node: &Node, fmt: &mut dyn Renderer) {
    // `page` 必填。没写就是用法错误 —— 那是 `render_problem` 的场合，不是解锁框的
    let Some(page) = template
        .param("page")
        .map(str::trim)
        .filter(|page| !page.is_empty())
    else {
        render_problem(
            template,
            fmt,
            "缺少 page=…：::decrypt 要指出要解锁的是哪一页",
        );
        return;
    };

    match crate::markdown::template_page(page) {
        // 读不动 → 解锁框
        Some(crate::markdown::TemplatePage::Unreadable { reason, protection }) => {
            render_decrypt_box(
                template,
                node,
                fmt,
                &Unreadable {
                    page: page.to_string(),
                    reason,
                    needs_passphrase: protection.symmetric,
                },
            );
        }
        // 读得动 → 底部内容（空则一句"已解锁"）
        Some(crate::markdown::TemplatePage::Ready(_)) => {
            if node.children.is_empty() {
                render_decrypt_done(fmt, page);
                return;
            }
            // 与别的模板同一套做法：块内容是**一个 markdown 子页面**，
            // 里面再嵌 `::decrypt` 就照常渲染（作者说的"自然渲染出来是什么就是什么"）
            fmt.cr();
            crate::markdown::deeper(|| fmt.contents(&node.children));
            fmt.cr();
        }
        // 没有这一页
        None => render_problem(
            template,
            fmt,
            &format!("`{page}` 这一页不存在（页名按页面名写）"),
        ),
    }
}

/// 读不出来的那一页，渲染时需要的三样
struct Unreadable {
    /// 页名（写进 `data-decrypt-title`）
    page: String,
    /// 为什么读不出来（原文，给人看）
    reason: String,
    /// 对称层为真；gpg 层为假 —— 它问的是钥匙串或智能卡
    needs_passphrase: bool,
}

/// 摆解锁框。
///
/// 后端只出**标记**，输入框与按钮由前端 `dom/decrypt.ts` 的 `decryptBox` 造 ——
/// 同样的框还有另一个来路（加密附件），两边共用那一个构造器，框的样子才只有一处定义。
/// 钉它的测试在 `expand.rs`：
/// `the_unlock_box_is_only_a_marker_so_both_callers_share_one_builder`。
fn render_decrypt_box(
    template: &Template,
    node: &Node,
    fmt: &mut dyn Renderer,
    locked: &Unreadable,
) {
    fmt.cr();
    fmt.open(
        "div",
        &[("class", "template template--decrypt".to_string())],
    );
    fmt.cr();
    fmt.open("p", &[("class", "template__head".to_string())]);
    fmt.open("span", &[("class", "template__badge".to_string())]);
    fmt.text("这一页还锁着");
    fmt.close("span");
    fmt.close("p");
    fmt.cr();
    fmt.open(
        "span",
        &[
            ("class", "decrypt".to_string()),
            ("data-decrypt", "yes".to_string()),
            ("data-decrypt-kind", "page".to_string()),
            ("data-decrypt-title", locked.page.clone()),
            (
                "data-decrypt-label",
                template.param("label").unwrap_or(&locked.page).to_string(),
            ),
            ("data-decrypt-reason", locked.reason.clone()),
            (
                "data-decrypt-needs-passphrase",
                if locked.needs_passphrase { "yes" } else { "no" }.to_string(),
            ),
        ],
    );
    fmt.close("span");
    fmt.cr();
    // **下一步**：指向那一页的链接。`internal` 是解析器自己摆的那种占位
    // （`expand.rs` 的 children 上挂的就是它），作者亲手写的 `::decrypt`
    // 没有这个 —— 它的 children 是底部内容，锁着的时候不该显示出来。
    if template.param("internal").is_some() {
        fmt.open("p", &[("class", "template__where".to_string())]);
        fmt.contents(&node.children);
        fmt.close("p");
        fmt.cr();
    }
    fmt.close("div");
    fmt.cr();
}

/// 已经解开了、而 `::decrypt` 底部又是空的：摆一句"已解锁"。
///
/// 刻意**不删掉这个位置** —— 作者写了 `::decrypt page=X` 就是要在这里占一个位，
/// 悄悄消失的话，正文会突然少一块，而人不知道那是被程序吃掉了还是自己写错了。
fn render_decrypt_done(fmt: &mut dyn Renderer, page: &str) {
    fmt.cr();
    fmt.open(
        "div",
        &[
            ("class", "decrypt decrypt--done".to_string()),
            ("data-decrypt-done", "yes".to_string()),
        ],
    );
    fmt.open("span", &[("class", "decrypt__badge".to_string())]);
    fmt.text("已解锁");
    fmt.close("span");
    fmt.open("span", &[("class", "decrypt__label".to_string())]);
    fmt.text(page);
    fmt.close("span");
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
