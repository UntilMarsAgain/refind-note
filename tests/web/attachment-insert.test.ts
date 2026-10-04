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
 * `composables/useAttachmentInsert.ts` 里那句"这里没法输口令"的判断。
 *
 * 这个 composable 本体要 DOM 与 Tauri 插件，测不了；被抠出来的正是**纯逻辑**那一半，
 * 也正是这个坑的全部 —— 见那个函数上的说明。
 */

import assert from "node:assert/strict";
import { describe, it } from "node:test";

import { missingPassphrase } from "../../src/composables/useAttachmentInsert.ts";
import type { Policy } from "../../src/ipc/note.ts";

const plain: Policy = {
    compress: true,
    compression: "deflate",
    gpg_sign: null,
    gpg_encrypt: null,
    symmetric: false,
    cipher: "aes-256-gcm",
};

const locked: Policy = { ...plain, symmetric: true };

describe("仓库默认不要口令时", () => {
    it("不用拦", () => {
        assert.equal(missingPassphrase(plain, ""), null);
    });

    it("给了策略也一样不用拦", () => {
        assert.equal(missingPassphrase(undefined, ""), null);
    });

    it("用户输没输都无所谓（没套口令层，多余的口令没人要）", () => {
        assert.equal(missingPassphrase(plain, "1234"), null);
    });
});

describe("仓库默认套了口令层时", () => {
    it("没有口令就拦下来，并说清去哪儿输", () => {
        const message = missingPassphrase(locked, "");
        assert.ok(message, "该拦下来");
        // 关键是那句"这里没法输口令"要指向一个真的能输的地方
        assert.match(message, /口令/);
        assert.match(message, /文件/);
    });

    it("输过了就放行", () => {
        assert.equal(missingPassphrase(locked, "1234"), null);
    });
});