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
 * 每个标签页自己的**编辑状态**：正在写的字、光标在哪、编辑区滚到哪。
 *
 * 为什么要有这一份：标签页记的是"在哪一页"（`TabState`），而编辑器一卸载，
 * 里面那些**没提交的字、光标和视线**就没了 —— 切去别的标签页再切回来，
 * 看到的是一段空白的编辑器。哪怕草稿槽位里存着备份，也得人点一下"恢复"，
 * 而且光标与滚动位置回不来。
 *
 * 所以这一份按**标签页 id** 存在内存里，与草稿槽位分工明确：
 *
 * - 这里：**切走再切回来，原位原样还在**（字、光标、滚动）；
 * - 草稿槽位（`save_draft`）：**兜底** —— 崩了、断电了、关掉标签页之后，它还认。
 *
 * 于是：有这一份时就不拿草稿去问人（草稿是定时落盘的，比这一份旧）；
 * 没有时才按老规矩摆出"恢复未提交的草稿"。
 *
 * 关掉标签页时忘掉它（见 [`forget`]）—— 那之后就只剩草稿槽位那一份了。
 */

export interface EditingState {
    /** 编辑器里的字 */
    markdown: string;
    /** 光标（文档里的偏移） */
    cursor: number;
    /** 编辑区滚到哪 */
    scroll: number;
}

const states = new Map<string, EditingState>();

/** 记下这个标签页此刻的样子 */
export function remember(tabId: string, state: EditingState): void {
    states.set(tabId, state);
}

/** 这个标签页上一回是什么样子（没有就是没在编辑过） */
export function recall(tabId: string | undefined): EditingState | undefined {
    return tabId ? states.get(tabId) : undefined;
}

/** 忘掉它（关标签页时） */
export function forget(tabId: string): void {
    states.delete(tabId);
}
