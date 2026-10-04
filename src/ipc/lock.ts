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
 * 页内解锁 —— 与 Rust 侧 `src-tauri/src/commands/lock.rs` 一一对应。
 *
 * 页内读不出来的份有两条来路 —— 被 `::` 引用的模板页、加密附件 —— 它们都摆同一个
 * 解锁框，所以后端也只给一条命令。字段名是 snake_case：Rust 那侧没有全局 serde
 * rename，改这边就要改那边，而改错了一边是**静默**的（收到 `undefined`，而 `undefined`
 * 是假值）。Rust 侧有测试钉住这一点。
 */

import { invoke } from "@tauri-apps/api/core";

/** 解锁框的种类 */
export type LockKind = "page" | "file";

/**
 * 一份东西现在的锁状态。
 *
 * `readable` 与 `needs_unlock` 是两件事，别混：前者是**真的去读了一次**才知道的，
 * 后者是"明文头里写着要解锁"。gpg 那一层两者会不一致 —— 头说要解锁，而真读的时候
 * 才发现没有私钥。正因为如此，界面必须听 `readable` 的，不能听 `needs_unlock` 的。
 */
export interface LockState {
    kind: LockKind;
    /** 页内解锁框拿它去重新问（`unlock` 与 `read` 都按标题走） */
    title: string;
    /** 仓库里有没有这一份。为假时后面几个字段都没意义 */
    exists: boolean;
    /** 要不要先解锁才能读（明文头里写着） */
    needs_unlock: boolean;
    /** 需要口令（对称层）。gpg 那一层为假 —— 它问的是钥匙串 */
    needs_passphrase: boolean;
    /** gpg 加密的（有没有私钥真要读的时候才知道） */
    needs_secret_key: boolean;
    /** 口令正躺在本次会话里 */
    passphrase_ready: boolean;
    /** **真的去读过了，读得动吗** */
    readable: boolean;
    /** 读不动时为什么（原文，给人看） */
    reason: string;
}

/**
 * 交口令（可省）→ 真的去读一次 → 报告读得动读不动。
 *
 * 口令错了**不**抛错，而是 `readable: false` + `reason`：解锁框要留在原地，
 * 让人改了口令再按一次。
 */
export async function lockState(
    kind: LockKind,
    title: string,
    reference?: string | null,
    passphrase?: string,
): Promise<LockState> {
    return await invoke<LockState>("lock_state", { kind, title, reference, passphrase });
}