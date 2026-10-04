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
 * 加密文件的解锁。
 *
 * 文件进了 blob 仓，就与笔记一样可能带口令层或 gpg 加密层 —— 于是"把这张图显示出来"
 * 这件事不再是无条件的。
 *
 * 这里只提供**看头**的那一问（`fileInfo`，不读字节）与**解锁之后绕开缓存**的那一手
 * （`freshUrl`）。判断"要不要摆解锁框"**不在这里** —— 它在 `<img>` 真的加载失败
 * 之后才做，见 `dom/note-html.ts` 的 `markMissing`。
 *
 * 原来这里还有一个 `readable()`，拿头去猜"读不读得动"，连同 `unlockFile` 一起删了：
 *
 * - `readable()` 对 gpg 只能猜（本机有没有私钥、代理答不答应，猜不出来），而猜出来的
 *   后果是**白摆一个框**（gpg 大部分时候是自动的）或**该摆不摆**（人被卡住）。
 *   现在改成"先试，失败再问"，不问预测值。
 * - `unlockFile` 只交口令就收工，证明不了成不成；`ipc/lock.ts` 的 `resolveDecrypt`
 *   交完口令会**真的加载一次**，那才是能作数的那一步。
 */

import { invoke } from "@tauri-apps/api/core";
import type { FileInfo } from "../ipc/files.ts";

/** 问一份文件的现状（不读字节，只看明文头与本次会话里有没有口令） */
export async function fileInfo(name: string): Promise<FileInfo | null> {
    try {
        // 正文里引用的都是**最新一版**，所以 reference 明确给 null
        return await invoke<FileInfo>("file_info", { key: name, reference: null });
    } catch {
        // 不是文件、或者这一页不存在：调用方按"没有这回事"处理
        return null;
    }
}

// 原来这里有个 `unlockFile(title, passphrase)`：只把口令交给后端就算完。
//
// 它被 `ipc/lock.ts` 的 `lockState` **取代**了，因为"交完口令"不等于"读得动" ——
// gpg 那一层根本没有口令，交完什么也证明不了，于是失败只体现在后面那次 404 上，
// 表现成"图片不存在"且无法重试（见 `dom/decrypt.ts` 抬头那段）。
// `lockState` 交完口令会**真的去读一次**，所以那一步能说话。
// 现在它没有调用方，留着只会让人以为"交口令"就够。

/** 带一个"这次是新读的"后缀：解锁之后要让 webview 重新去取，而不是吃缓存 */
export function freshUrl(url: string): string {
    // 地址里可能已经带了查询（`?rev=3` —— 看的是历史里的某一版），那就用 `&` 接上
    const separator = url.includes("?") ? "&" : "?";
    return `${url}${separator}t=${Date.now()}`;
}
