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
 * 这是在 `::decrypt` 上**提交之后**调的那一条：交口令（可省）→ 后端真的去加载一次
 * → 报告成不成。
 *
 * ## 为什么"成不成"只能问后端
 *
 * gpg 那一层没有口令，也没有"先试试看"——它要么被 gpg-agent 悄悄解开（钥匙的口令
 * 有缓存、或者智能卡碰一下就行），要么失败，而失败之前什么迹象都没有。渲染期还
 * 是**故意不问**的（一篇笔记会被渲染很多次，每次弹一个 pinentry 窗口是荒唐的）。
 *
 * 所以"到底行不行"只能由人按一下那个按钮来问。这条命令就是那一次问。
 *
 * 字段名是 snake_case：Rust 那边没有全局 serde rename，改一边就要改两边，而改错了
 * 是**静默**的（收到 `undefined`，而 `undefined` 是假值）。Rust 侧有测试钉住。
 */

import { invoke } from "@tauri-apps/api/core";

/** 解锁框的种类 */
export type LockKind = "page" | "file";

/** 真的加载一次之后的结果 */
export interface ResolveResult {
    kind: LockKind;
    /** 解锁框拿它去重新问 */
    title: string;
    /** 读成了没有 */
    readable: boolean;
    /** 读不成时为什么（原文，给人看） */
    reason: string;
    /** 刚才是"口令不对"——界面据此说"再输一次"，而不是让人对着没反应的输入框发呆 */
    wrong_passphrase: boolean;
    /** 要不要给口令输入框。对称层为真；gpg 层为假——它问的是钥匙串或智能卡 */
    needs_passphrase: boolean;
}

/**
 * 交口令（可省）→ 后端真的加载一次 → 成或不成。
 *
 * **口令错了不抛**：`readable: false` 加一句 `reason`。解锁框要留在原地让人改了
 * 再按一次，抛错的话前端只知道"失败了"，得另外约定怎么区分"口令不对"与
 * "这份东西坏了"。
 *
 * 成功时**不返回内容**——由界面自己重读。后端要把三种上下文（笔记、页内嵌着的
 * 模板页、附件）都算成一段 HTML 塞回来，就得替每种各写一遍，而模板那一路注定是
 * 错的：`::卡片` 的参数是调用点给的，只看模板页本身算不出来。
 */
export async function resolveDecrypt(
    kind: LockKind,
    title: string,
    reference?: string | null,
    passphrase?: string,
): Promise<ResolveResult> {
    return await invoke<ResolveResult>("resolve_decrypt", {
        kind,
        title,
        reference,
        passphrase,
    });
}