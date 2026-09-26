//! 历史与差异：与 Rust 端 `RevisionSummary` / `DiffLine` / `DiffResult` 一一对应。
//!
//! 手写（不生成）：改了 Rust 那边就改这里，然后让 `pnpm build` 指出要跟着改的地方。

export interface RevisionSummary {
  rev: number;
  /** create / commit / draft / delete */
  kind: string;
  id: string;
  short_id: string;
  at: string;
  bytes: number;
  delta: number;
  supersedes: number[];
  summary: string | null;
}

/** `DiffLine` */

export interface DiffLine {
  kind: string;
  old_line: number | null;
  new_line: number | null;
  text: string;
}

/** `DiffResult`：两版之间 */

export interface DiffResult {
  from_rev: number;
  to_rev: number;
  from_title: string;
  to_title: string;
  lines: DiffLine[];
  inserted: number;
  deleted: number;
}

/** `VaultSettings`：设置页与外观都读它（仓库级设置 + 外观 + 维护时间） */
