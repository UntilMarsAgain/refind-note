//! 笔记本身：与 Rust 端 `Note` / `NoteSummary` / `LoadOutcome` / `Draft` /
//! `RevisionContent` 一一对应。
//!
//! 手写（不生成）：改了 Rust 那边就改这里，然后让 `pnpm build` 指出要跟着改的地方。

import type { CommandInfo } from "./command";

export interface Note {
  key: string;
  title: string;
  markdown: string;
  html: string;
  rev: number;
  modified: string;
}

/** `LoadOutcome`：目标不存在不算错误，而是交给界面处理 */

export interface NoteSummary {
  key: string;
  title: string;
  /** 指令信息；普通页面是 null */
  command: CommandInfo | null;
}

/** `CommandInfo`：一条指令页面的标注，全部来自后端那张指令表 */

export interface LoadOutcome {
  note: Note | null;
  title: string;
  deleted: boolean;
}

/** `Draft`：最新的未提交草稿 */

export interface Draft {
  markdown: string;
  base_rev: number;
  at: string;
  /** 草稿也是链上的一版，所以也有 commit ID —— 预览走的就是它 */
  id: string;
  short_id: string;
}

/** `RevisionContent`：历史里某一版的正文 */

export interface RevisionContent {
  rev: number;
  kind: string;
  at: string;
  title: string;
  markdown: string;
  html: string;
}

/** `RevisionSummary`：历史列表里的一版 */
