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
 * `core/address.ts` 的那四个纯函数：标签名、章节、规范串的回显。
 *
 * 前端**不自己拼地址**（规范串由后端给），所以这里只有"从后端给的东西里取出显示用的部分"。
 */

import assert from "node:assert/strict";
import { describe, it } from "node:test";

import type { ResolvedAddress } from "../../src/ipc/address.ts";
import { canonicalOf, sectionOf, titleOf, withSection } from "../../src/core/address.ts";

/** 造一个 `resolve_address` 的产物，只填用得上的那几个字段 */
function route(outcome: ResolvedAddress["outcome"], section = ""): ResolvedAddress {
    return {
        address: {
            namespace: { id: "0", spelling: "" },
            page: "示例",
            mode: { kind: "view", ref: null },
            section,
        },
        canonical: "示例",
        outcome,
        editable: true,
        via: null,
    };
}

describe("标签栏上显示的名字", () => {
    it("笔记 / 不存在 / 帮助 / 文件都用后端给的标题", () => {
        assert.equal(titleOf(route({ kind: "note", title: "甲" })), "甲");
        assert.equal(titleOf(route({ kind: "missing", title: "乙" })), "乙");
        assert.equal(titleOf(route({ kind: "help", page: "首页", title: "首页" })), "首页");
        assert.equal(titleOf(route({ kind: "file", title: "桥.png" })), "桥.png");
        assert.equal(
            titleOf(route({ kind: "cross-site", title: "外站", url: "https://x.invalid" })),
            "外站",
        );
    });

    it("特殊页用登记过的显示名", () => {
        assert.equal(titleOf(route({ kind: "special", page: "settings" })), "设置");
        assert.equal(titleOf(route({ kind: "special", page: "trash" })), "回收站");
    });
});

describe("规范串", () => {
    it("没有落地结果时给空串", () => {
        assert.equal(canonicalOf(null), "");
        assert.equal(sectionOf(null), "");
    });

    it("有落地结果时就是后端那一串", () => {
        assert.equal(canonicalOf(route({ kind: "note", title: "甲" })), "示例");
    });
});

describe("章节", () => {
    it("取地址里带的那一段", () => {
        assert.equal(sectionOf(route({ kind: "note", title: "甲" }, "小节")), "小节");
    });

    it("换章节：只动 `#` 之后的部分，前缀原样保留", () => {
        assert.equal(withSection("示例", "小节"), "示例#小节");
        assert.equal(withSection("命名空间:示例@view", "小节"), "命名空间:示例@view#小节");
    });

    it("换章节：原有的 `#` 被替换而不是叠上去", () => {
        assert.equal(withSection("示例#旧的", "新的"), "示例#新的");
    });

    it("给空章节就是去掉 `#` 之后那一段", () => {
        assert.equal(withSection("示例#旧的", ""), "示例");
    });

    it("往返一次还是原来那串（点锚点时靠这个不写坏地址）", () => {
        const canonical = "命名空间:示例@view#小节";
        assert.equal(withSection(canonical, sectionOf(route({ kind: "note", title: "甲" }, "小节"))), canonical);
    });
});