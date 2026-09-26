//! 后台任务：与 Rust 端 `tasks::Task` 一一对应。
//!
//! 手写（不生成）：改了 Rust 那边就改这里，然后让 `pnpm build` 指出要跟着改的地方。

export interface Task {
  id: number;
  label: string;
  state: "queued" | "running" | "done" | "failed";
  submitted_at: string;
  finished_at: string;
  message: string;
}
