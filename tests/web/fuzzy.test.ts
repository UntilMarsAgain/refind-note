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

/** `core/fuzzy.ts`：模糊搜索的打分与次序（见那个文件头的四档说明） */

import assert from "node:assert/strict";
import { describe, it } from "node:test";

import { fuzzySearch } from "../../src/core/fuzzy.ts";

/** 搜标题 */
const titles = (query: string, items: string[]): string[] =>
    fuzzySearch(query, items, (item) => item);

describe("空查询", () => {
    it("原样返回全部，保持原次序", () => {
        const items = ["乙", "甲", "丙"];
        assert.deepEqual(titles("   ", items), items);
    });

    it("不给候选时给空数组", () => {
        assert.deepEqual(titles("甲", []), []);
    });
});

describe("四档打分", () => {
    it("完全匹配排在包含之前", () => {
        assert.deepEqual(titles("卡片", ["我的卡片集", "卡片"]), ["卡片", "我的卡片集"]);
    });

    it("前缀匹配排在中间包含之前", () => {
        assert.deepEqual(titles("卡片", ["我的卡片集", "卡片模板"]), ["卡片模板", "我的卡片集"]);
    });

    it("前缀里越短的越贴（`卡片` 该在 `卡片模板` 前面）", () => {
        assert.deepEqual(titles("卡片", ["卡片模板", "卡片"]), ["卡片", "卡片模板"]);
    });

    it("子序列能命中，但排在包含之后", () => {
        const items = ["我的键盘笔记", "键盘笔记"];
        assert.deepEqual(titles("键盘笔", items), ["键盘笔记", "我的键盘笔记"]);
    });

    it("顺序不对就不是子序列", () => {
        // `卡` 在 `片` 后面：这不是"按顺序能对上"
        assert.deepEqual(titles("片卡", ["卡片"]), []);
    });
});

describe("大小写与空白", () => {
    it("大小写不影响匹配", () => {
        assert.deepEqual(titles("card", ["Card", "board"]), ["Card"]);
    });

    it("查询两边的空白不算", () => {
        assert.deepEqual(titles("  卡片 ", ["卡片"]), ["卡片"]);
    });
});

describe("同分时的稳定性", () => {
    it("同等相关度按原次序，不随排序实现乱跳", () => {
        const items = ["备注一", "备注二", "备注三"];
        assert.deepEqual(titles("备注", items), items);
    });

    it("同分且同档时也按原次序", () => {
        // 两个都是"前缀命中、长度相同" → 同分，只能靠原次序分出前后
        const items = ["ab", "ac"];
        assert.deepEqual(titles("a", items), items);
    });
});

describe("泛型", () => {
    it("取出来的就是原来那些对象", () => {
        type Row = { title: string; at: number };
        const rows: Row[] = [
            { title: "卡片", at: 1 },
            { title: "别的东西", at: 2 },
        ];
        const hit = fuzzySearch("卡片", rows, (row) => row.title);
        assert.deepEqual(hit, [rows[0]]);
        assert.equal(hit[0]?.at, 1);
    });

    it("命中的每一项都能取出拿来比的那个字符串", () => {
        // 只要求 `textOf` 取得出字符串：故意传一个没有索引签名的接口，
        // 与 `fuzzy.ts` 里那个约束一致（设了索引签名反而会把这类类型挡掉）
        interface ListRow {
            title: string;
        }
        const rows: ListRow[] = [{ title: "读我" }, { title: "略过" }];
        assert.deepEqual(
            fuzzySearch("读", rows, (row) => row.title).map((row) => row.title),
            ["读我"],
        );
    });
});