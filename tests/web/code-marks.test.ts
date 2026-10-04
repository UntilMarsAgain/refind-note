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

/** `core/markdown/code-marks.ts`：`highlight=3,5-7` 这种小语法的解析 */

import assert from "node:assert/strict";
import { describe, it } from "node:test";

import { markedLines, parseLineRanges } from "../../src/core/markdown/code-marks.ts";

describe("解析区间", () => {
    it("单行就是从它到它", () => {
        assert.deepEqual(parseLineRanges("3"), [{ from: 3, to: 3 }]);
    });

    it("区间含两端", () => {
        assert.deepEqual(parseLineRanges("5-7"), [{ from: 5, to: 7 }]);
    });

    it("`4-` 表示到最后一行", () => {
        assert.deepEqual(parseLineRanges("4-"), [{ from: 4, to: null }]);
    });

    it("逗号分隔，按写的顺序", () => {
        assert.deepEqual(parseLineRanges("3,5-7,9"), [
            { from: 3, to: 3 },
            { from: 5, to: 7 },
            { from: 9, to: 9 },
        ]);
    });

    it("空白随便写", () => {
        assert.deepEqual(parseLineRanges(" 3 , 5 - 7 "), [
            { from: 3, to: 3 },
            { from: 5, to: 7 },
        ]);
    });

    it("空串没有区间（模板不写 `highlight` 时走这里）", () => {
        assert.deepEqual(parseLineRanges(""), []);
        assert.deepEqual(parseLineRanges("   "), []);
    });
});

describe("乱写的地方跳过，不整条作废", () => {
    it("一个笔误不该让整块代码失去高亮", () => {
        assert.deepEqual(parseLineRanges("3,笔误,5"), [
            { from: 3, to: 3 },
            { from: 5, to: 5 },
        ]);
    });

    it("行号从 1 起：0 与负数不算", () => {
        assert.deepEqual(parseLineRanges("0"), []);
        assert.deepEqual(parseLineRanges("-3"), []);
    });

    it("上界小于下界的区间不算（写反了不算数）", () => {
        assert.deepEqual(parseLineRanges("7-5"), []);
    });

    it("空的尾巴（`5-`）与写反了（`5-abc`）是两回事", () => {
        assert.deepEqual(parseLineRanges("5-"), [{ from: 5, to: null }]);
        assert.deepEqual(parseLineRanges("5-abc"), []);
    });
});

describe("展开成行号", () => {
    it("升序、去重", () => {
        assert.deepEqual(markedLines("5,3,3", 10), [3, 5]);
    });

    it("展开区间", () => {
        assert.deepEqual(markedLines("2-4", 10), [2, 3, 4]);
    });

    it("`4-` 到最后一行，但不超过实际行数", () => {
        assert.deepEqual(markedLines("4-", 6), [4, 5, 6]);
        // 整块只有 3 行，从第 4 行开始标 = 一行都标不出来（不是"标到最后一行"）
        assert.deepEqual(markedLines("4-", 3), []);
    });

    it("整块都在范围内时就全标上", () => {
        assert.deepEqual(markedLines("1-", 3), [1, 2, 3]);
    });

    it("行号从 1 起，与 `start=` 无关（作者写的是屏幕上看到的号）", () => {
        // 这块代码从第 10 行开始显示，但 `highlight` 里的 1 就是第一行
        assert.deepEqual(markedLines("1-2", 3), [1, 2]);
    });

    it("一行都没有（空参数或全乱写）", () => {
        assert.deepEqual(markedLines("", 10), []);
        assert.deepEqual(markedLines("乱写", 10), []);
    });
});