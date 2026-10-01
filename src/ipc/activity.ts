/**
 * 最近编辑与浏览历史 —— 与 Rust 侧 `src-tauri/src/changes.rs`、`browsing.rs` 对应。
 */

/** 流水里的一条 */
export interface ChangeEntry {
    title: string;
    /** `commit`（提交）/ `delete`（删除）/ `draft`（还没提交的草稿） */
    kind: string;
    rev: number;
    at: string;
    bytes: number;
    delta: number;
}

/** 看过的一页 */
export interface Visit {
    /** 当时地址栏里那一串（含状态与章节） */
    address: string;
    title: string;
    at: string;
}
