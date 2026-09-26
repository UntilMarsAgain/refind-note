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
