//! 诊断报告：与 Rust 端 `DebugReport` / `DebugSection` / `DebugEntry` 一一对应。
//!
//! 手写（不生成）：改了 Rust 那边就改这里，然后让 `pnpm build` 指出要跟着改的地方。

/** 报告里的一行：一个标签配一个值（值可以是多行） */
export interface DebugEntry {
  label: string;
  value: string;
}

/** 报告里的一段，对应界面上的一张小表 */
export interface DebugSection {
  title: string;
  entries: DebugEntry[];
}

/** 整份报告（`special:debug`） */
export interface DebugReport {
  sections: DebugSection[];
}

/** 编辑页「状态」面板要的编译报告：这次渲染到底发生了什么 */
export interface RenderReport {
  /** 送进渲染器的文本规模 */
  markdown_bytes: number;
  markdown_lines: number;
  /** 渲染出的 HTML */
  html: string;
  html_bytes: number;
  /** 渲染耗时（毫秒） */
  millis: number;
  /** 这一页按什么语言对待（`css` / `html` / markdown） */
  language: string;
  /** 正文里每条模板块会走哪条分发路径 */
  blocks: DebugEntry[];
}
