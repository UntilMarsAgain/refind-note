//! 回收站条目：与 Rust 端 `TrashEntry` 一一对应。
//!
//! 手写（不生成）：改了 Rust 那边就改这里，然后让 `pnpm build` 指出要跟着改的地方。

export interface TrashEntry {
  title: string;
  deleted_at: string;
  bytes: number;
  /** `null` = 删除时间读不出来（会列出，但不参与自动清理） */
  days_old: number | null;
}

/** `Task`：后台任务 */
