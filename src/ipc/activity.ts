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
