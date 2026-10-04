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

/** `core/outline.ts`：把一份平的标题列表变成可点的目录 */

import assert from "node:assert/strict";
import { describe, it } from "node:test";

import {
    MIN_HEADINGS,
    activeOf,
    outlineOf,
    shouldShowOutline,
    type Heading,
} from "../../src/core/outline.ts";

/** 造一份标题：只写关心的那几个字段 */
const h = (id: string, level: number, text: string): Heading => ({ id, level, text });

describe("顺序", () => {
    it("就是正文里的次序，不另按字母排", () => {
        const entries = outlineOf([h("z", 1, "第一"), h("a", 2, "第二"), h("m", 2, "第三")]);
        assert.deepEqual(
            entries.map((entry) => entry.text),
            ["第一", "第二", "第三"],
        );
    });
});

describe("缩进按这一篇实际用到的那几级算", () => {
    it("从 h1 一路到 h3 就是 0、1、2", () => {
        const entries = outlineOf([h("a", 1, "甲"), h("b", 2, "乙"), h("c", 3, "丙")]);
        assert.deepEqual(
            entries.map((entry) => entry.depth),
            [0, 1, 2],
        );
    });

    it("从 h2 开始的笔记，第一层就是 0（不留三格空白）", () => {
        const entries = outlineOf([h("a", 2, "甲"), h("b", 3, "乙")]);
        assert.deepEqual(
            entries.map((entry) => entry.depth),
            [0, 1],
        );
    });

    it("跳级不算笔误，按实际用到的那几级压紧", () => {
        // `#` 直接到 `####` 在深层笔记里很常见；照 level 缩进会空出三格
        const entries = outlineOf([h("a", 1, "甲"), h("b", 4, "乙"), h("c", 6, "丙")]);
        assert.deepEqual(
            entries.map((entry) => entry.depth),
            [0, 1, 2],
        );
    });

    it("先出现深的再出现浅的也算（次序按正文，层级按出现过的级）", () => {
        const entries = outlineOf([h("a", 3, "甲"), h("b", 1, "乙"), h("c", 3, "丙")]);
        assert.deepEqual(
            entries.map((entry) => entry.depth),
            [1, 0, 1],
        );
    });
});

describe("滤掉不能点的那些", () => {
    it("空的标题不列（列出来是个点不开的条目）", () => {
        const entries = outlineOf([h("a", 1, "甲"), h("b", 2, "   "), h("c", 2, "丙")]);
        assert.deepEqual(
            entries.map((entry) => entry.text),
            ["甲", "丙"],
        );
    });

    it("没有 `id` 的不列（`::html` 里手写的标题不一定有）", () => {
        const entries = outlineOf([h("a", 1, "甲"), h("", 2, "乙")]);
        assert.deepEqual(
            entries.map((entry) => entry.text),
            ["甲"],
        );
    });

    it("一条都没有时给空数组", () => {
        assert.deepEqual(outlineOf([]), []);
    });
});

describe("文字收拾干净", () => {
    it("换行与多余空白压成单空格（渲染出来的标题可能带着换行）", () => {
        const entries = outlineOf([h("a", 1, "  甲\n  乙  ")]);
        assert.equal(entries[0]?.text, "甲 乙");
    });

    it("首尾空白去掉", () => {
        assert.equal(outlineOf([h("a", 1, " 甲 ")])[0]?.text, "甲");
    });
});

describe("够不够格摆一个目录", () => {
    it("三条以内不摆（那份清单比正文还长）", () => {
        const three = outlineOf([h("a", 1, "甲"), h("b", 2, "乙"), h("c", 2, "丙")]);
        assert.equal(shouldShowOutline(three), false);
    });

    it("四条起就摆", () => {
        const four = outlineOf([
            h("a", 1, "甲"),
            h("b", 2, "乙"),
            h("c", 2, "丙"),
            h("d", 2, "丁"),
        ]);
        assert.equal(shouldShowOutline(four), true);
        assert.equal(MIN_HEADINGS, 4);
    });

    it("空标题不算数（凑不到四条就不摆）", () => {
        const padded = outlineOf([
            h("a", 1, "甲"),
            h("b", 2, "乙"),
            h("c", 2, ""),
            h("d", 2, "  "),
        ]);
        assert.equal(shouldShowOutline(padded), false);
    });
});

describe("原样带下去的东西", () => {
    it("`id` 与 `level` 一字不改（滚过去靠 id，缩进才用得上 level）", () => {
        const entries = outlineOf([h("标题-一", 2, "甲")]);
        assert.equal(entries[0]?.id, "标题-一");
        assert.equal(entries[0]?.level, 2);
    });
});
describe("滚到哪儿了，该亮着哪一条", () => {
    it("一个都没读过去时不亮（视口上沿还没到第一个标题）", () => {
        assert.equal(activeOf([200, 400, 600], 96), -1);
    });

    it("读过了第一个就亮第一个", () => {
        assert.equal(activeOf([0, 400, 600], 96), 0);
    });

    it("一屏里有两个标题时取**刚越过的最后那个**，不跳", () => {
        // 0 与 50 都已经越过视口上沿：亮第二个
        assert.equal(activeOf([0, 50, 600], 96), 1);
    });

    it("停在标题上方一点点时还是上一条（刚够到不算读过）", () => {
        assert.equal(activeOf([0, 97, 600], 96), 0);
    });

    it("滚到底部时亮最后一条（上边距已经全是负的 —— 都读过去了）", () => {
        // 量的是相对滚动容器上沿的位置：往下滚，之前那些就成了负数
        assert.equal(activeOf([0, -400, -900], 96), 2);
    });

    it("没有条目时是 -1（不是 0 —— 那会把第一条点亮）", () => {
        assert.equal(activeOf([], 96), -1);
    });

    it("量不出来的（`Infinity`）不算读过，它在正文里已经不在了", () => {
        assert.equal(activeOf([0, Number.POSITIVE_INFINITY], 96), 0);
    });

    it("空数组与阈值 0 也不出错", () => {
        assert.equal(activeOf([], 0), -1);
        assert.equal(activeOf([0], 0), 0);
    });
});
