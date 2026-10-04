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
 * 这件事不再是无条件的：**先问后端"这一版怎么存的、现在读不读得动"**，
 * 读不动就摆一个解锁按钮，而不是让 `<img>` 去撞一堵 404 的墙。
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

/**
 * 这一份**已经**读得动吗（不是"大概读得动"）。
 *
 * ## gpg 那一层不能乐观
 *
 * 原来这里写着「gpg 由系统代理管：直接去读就是了」→ 返回 true。于是
 * `attachVaultFile` **连框都不摆**，`<img>` 去撞 404，而 `platform::protocol`
 * 把"解密失败"与"没有这一份"一起答成 404，前端于是说"图片不存在" ——
 * 没有框，也就没有重试的机会。这是那个 bug 的**触发点**。
 *
 * 本机有没有那把私钥、代理答不答应，**只有真读才知道** —— 所以这里必须说"不知道"，
 * 由 `lockState` 去真的读一次（见 `dom/decrypt.ts`）。
 *
 * @see the_same_readable_verdict_for_every_layer_including_gpg
 */
export function readable(info: FileInfo): boolean {
    if (!info.needs_unlock) {
        return true;
    }
    // gpg 在场就是"不知道" —— 哪怕口令已经在手也一样：口令解开的是外面那层，
    // 里面还有一层要问钥匙串，而那把私钥在不在没人提前知道。
    if (info.needs_secret_key) {
        return false;
    }
    return info.passphrase_ready;
}

/** 带一个"这次是新读的"后缀：解锁之后要让 webview 重新去取，而不是吃缓存 */
export function freshUrl(url: string): string {
    // 地址里可能已经带了查询（`?rev=3` —— 看的是历史里的某一版），那就用 `&` 接上
    const separator = url.includes("?") ? "&" : "?";
    return `${url}${separator}t=${Date.now()}`;
}
