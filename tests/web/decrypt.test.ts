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
 * 页内解锁：`readable()` 那个判断，与 `boxShape()` 框的形状。
 *
 * 这里是**那个 bug 的回归测试**。它原来长这样：
 *
 *     if (!info.needs_unlock || readable(info)) return;
 *     // readable()：gpg 那一层"由系统代理管" → true
 *
 * 于是 gpg 加密的附件**连解锁框都不摆**，`<img>` 直接去撞 404，而
 * `platform::protocol` 把"解密失败"和"没有这一份"一起答成 404，前端于是说
 * "图片不存在"。没有框，也就没有重试的机会 —— 正是报告里说的那三样。
 *
 * 修法是：`readable()` 遇到 gpg 必须说"不知道"，由 `lockState` 去**真的读一次**。
 * 那一步只有后端做得了，所以这里测的是"它别再乐观"。
 */

import assert from "node:assert/strict";
import { describe, it } from "node:test";

import { boxShape } from "../../src/dom/decrypt.ts";
import { readable } from "../../src/dom/file-unlock.ts";
import type { FileInfo } from "../../src/ipc/files.ts";

/** 造一个 `file_info` 的产物，只填这个判断要用的字段 */
function info(over: Partial<FileInfo> = {}): FileInfo {
    return {
        title: "File:桥.png",
        name: "桥.png",
        mime: "image/png",
        size: 1,
        rev: 1,
        modified: "",
        url: "refind://localhost/file/%E6%A1%A5.png",
        // 这几项这个判断用不到；填**合法值**而不是 null ——
        // Rust 侧它们不是 Option（没压过时是 Deflate / Aes256Gcm），
        // 填 null 的话这个文件就成了"测试数据与真实形状不一致"，反而骗人。
        protection: {
            compress: false,
            compression: "deflate",
            sign: null,
            encrypt: null,
            symmetric: false,
            cipher: "aes-256-gcm",
        },
        needs_unlock: false,
        passphrase_ready: false,
        needs_passphrase: false,
        needs_secret_key: false,
        ...over,
    };
}

describe("readable 说不说「读得动」", () => {
    it("没加密的当然读得动", () => {
        assert.equal(readable(info()), true);
    });

    // ↓↓↓ 这三条是这个文件存在的理由 ↓↓↓
    it("gpg 加密的：说「不知道」，不能说「读得动」", () => {
        // 本机有没有那把私钥、代理答不答应，只有真读才知道。
        // 说"读得动"的后果是框都不摆，`<img>` 去撞 404，报成"图片不存在"。
        assert.equal(readable(info({ needs_unlock: true, needs_secret_key: true })), false);
    });

    it("gpg 加口令的，口令在手也说「不知道」", () => {
        // 口令解开的是外面那层，里面还有一层要问钥匙串。
        // 只看 `passphrase_ready` 就说读得动，于是解密失败又被报成"图片不存在"。
        assert.equal(
            readable(
                info({
                    needs_unlock: true,
                    needs_passphrase: true,
                    needs_secret_key: true,
                    passphrase_ready: true,
                }),
            ),
            false,
        );
    });

    it("纯口令的，口令在手才说读得动", () => {
        const locked = info({ needs_unlock: true, needs_passphrase: true });
        assert.equal(readable(locked), false, "还没口令就说读得动是错的");
        assert.equal(readable({ ...locked, passphrase_ready: true }), true);
    });
});

describe("解锁框的形状由「要不要口令」唯一决定", () => {
    it("口令层：给输入框，并且说清下一步", () => {
        const shape = boxShape(true);
        assert.equal(shape.showsInput, true);
        // 只说"这是加密的"的话人还是不知道自己能干什么
        assert.match(shape.hint, /口令/);
    });

    it("gpg 层：不给输入框（多给了也要看见，但那是错的形状）", () => {
        const shape = boxShape(false);
        assert.equal(shape.showsInput, false);
        // 不该提口令 —— gpg 问的是钥匙串，让人输口令是误导
        assert.doesNotMatch(shape.hint, /口令/);
    });

    it("两条路的按钮都叫同一个字", () => {
        assert.equal(boxShape(true).button, boxShape(false).button);
    });
});