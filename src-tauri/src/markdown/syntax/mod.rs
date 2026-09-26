//! 自定义语法。
//!
//! 约定：每个语法一个文件，对外暴露 `add(md)`；统一在 [`register`] 里挂载。
//! 新增一个语法 = 加一个文件 + 在 `register` 里加一行。

pub mod template;
pub mod wikilink;

use markdown_it::MarkdownIt;

/// 把所有自定义语法注册到解析器上。
pub fn register(md: &mut MarkdownIt) {
    template::add(md);
    wikilink::add(md);
}
