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
 * 页内解锁框的形状。
 *
 * ## 这个文件改过一次，原因记在这里
 *
 * 原来这里测的是一个叫 `readable()` 的函数：拿 `file_info` 的明文头去猜
 * "这一份读不读得动"，然后**提前**决定要不要摆解锁框。它有两处都是错的：
 *
 * ```
 * // gpg 那一层由系统代理管：直接去读就是了，读的时候它自己会问
 * if (info.needs_secret_key && !info.needs_passphrase) return true;
 * ```
 *
 * - **猜 gpg**："本机有没有那把私钥、代理答不答应"从明文头里**看不出来**。
 *   说"读得动" → 框都不摆，`<img>` 去撞 404，报成"图片不存在"且无法重试；
 *   说"读不动" → 白摆一个框，而 gpg 大部分时候是自动的（钥匙的口令有缓存、
 *   智能卡碰一下就行），人为了看一张图先得点一下"解锁"，点完它自己就解开了。
 * - **提前**：规格是"先试，失败了才摆框"（见 `dom/decrypt.ts` 抬头）。
 *
 * 所以 `readable()` 与 `unlockFile` **一起删了**，判断挪到两个真读一次的地方：
 * `<img>` 加载失败之后（`dom/note-html.ts` 的 `markMissing`）、以及附件详情页
 * 打开时（`composables/useFileDetail.ts`）。
 *
 * 留下来的是**框的形状** —— 那一部分与"猜不猜"无关，且它是纯函数、能测。
 */

import assert from "node:assert/strict";
import { describe, it } from "node:test";

import { boxShape } from "../../src/dom/decrypt.ts";

describe("解锁框的形状由「要不要口令」唯一决定", () => {
    it("口令层：给输入框，并且说清下一步", () => {
        const shape = boxShape(true);
        assert.equal(shape.showsInput, true);
        // 只说"这是加密的"的话人还是不知道自己能干什么
        assert.match(shape.hint, /口令/);
    });

    it("gpg 层：不给输入框，也不提口令", () => {
        const shape = boxShape(false);
        assert.equal(shape.showsInput, false);
        // 不该提口令 —— gpg 问的是钥匙串或智能卡，让人输口令是误导
        assert.doesNotMatch(shape.hint, /口令/);
    });

    it("两条路的按钮都叫同一个字", () => {
        // 分成两个字会让人以为不是一件事 —— 它们做的是同一件事：
        // 让后端真的去加载一次（`resolveDecrypt`），只是那一次里问的是谁不一样
        assert.equal(boxShape(true).button, boxShape(false).button);
    });

    it("形状真的由那一个布尔值决定，没有别的东西偷偷进来", () => {
        // 这一条钉的是"唯一"两个字：将来谁想按 gpg/口令之外的东西再分一次形状
        // （比如"这一版既对称又 gpg"），得在这里先说清楚为什么
        assert.deepEqual(Object.keys(boxShape(true)).sort(), ["button", "hint", "showsInput"]);
    });
});