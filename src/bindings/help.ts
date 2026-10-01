/**
 * 用户帮助 —— 与 Rust 侧 `src-tauri/src/features/help.rs` 一一对应。
 *
 * 正文是仓库 `help/` 目录下的 markdown，编译时就已经编进程序，运行时读不出来源文件。
 */

export interface HelpPage {
    /** 页面名（地址里写的那一段，例如 `Help:入门` 里的「入门」） */
    slug: string;
    /** 标题：正文第一个 `# ` 行 */
    title: string;
    /** 显示标题（`Help:入门`） */
    display: string;
    /** 正文源码 */
    markdown: string;
    /** 渲染好的 HTML */
    html: string;
}
