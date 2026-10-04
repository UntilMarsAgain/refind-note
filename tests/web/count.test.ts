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

/** `core/count.ts`：数一数写了多少 —— 字符、字、行 */

import assert from "node:assert/strict";
import { describe, it } from "node:test";

import { countOf, describeCount } from "../../src/core/count.ts";

describe("空文本", () => {
    it("三个数都是 0（不是 1 行）", () => {
        assert.deepEqual(countOf(""), { chars: 0, words: 0, lines: 0 });
    });
});

describe("字符数", () => {
    it("空白不算写了字", () => {
        assert.equal(countOf("甲 乙\n\t丙").chars, 3);
    });

    it("标点算字符", () => {
        assert.equal(countOf("甲，乙。").chars, 4);
    });

    it("emoji 算**一个** —— 不是 UTF-16 那套（`length` 会给 2）", () => {
        assert.equal(countOf("🧭").chars, 1);
        assert.equal("🧭".length, 2, "这条断言的前提就是 length 会数成 2");
    });
});

describe("字数：中文一字一算", () => {
    it("两个汉字是两个字", () => {
        assert.equal(countOf("水文").words, 2);
    });

    it("标点不算字", () => {
        assert.equal(countOf("水文，航道。").words, 4);
    });

    it("假名与韩文音节也是一字一算（写出来就是一个字）", () => {
        assert.equal(countOf("ひらがな").words, 4);
        assert.equal(countOf("한글").words, 2);
    });
});

describe("字数：拉丁与数字连着算一个", () => {
    it("一个英文单词是一个字（按字符数会数成六个，那没意义）", () => {
        assert.equal(countOf("refind").words, 1);
    });

    it("多个单词各算一个", () => {
        assert.equal(countOf("refind note").words, 2);
    });

    it("数字连着算一个（版本号 `0.1.0` 是三个）", () => {
        assert.equal(countOf("0.1.0").words, 3);
        assert.equal(countOf("v2").words, 1);
    });

    it("词里的连字符与撇号不断词", () => {
        assert.equal(countOf("well-known").words, 1);
        assert.equal(countOf("don't").words, 1);
    });

    it("标点会断词（`甲,乙` 在拉丁那边也是两个）", () => {
        assert.equal(countOf("ab,cd").words, 2);
    });
});

describe("混排", () => {
    it("两种规则各自生效", () => {
        // 三个汉字 + 两个英文单词
        const count = countOf("水文的 refind note");
        assert.equal(count.words, 5);
        assert.equal(count.chars, 3 + 6 + 4);
    });

    it("汉字与拉丁紧挨着不会粘成一个词", () => {
        assert.equal(countOf("水文refind").words, 2 + 1);
    });
});

describe("行数", () => {
    it("按换行切", () => {
        assert.equal(countOf("甲\n乙\n丙").lines, 3);
    });

    it("末尾那个换行**不再多算一行**", () => {
        assert.equal(countOf("甲\n").lines, 2);
    });

    it("有内容就是至少一行", () => {
        assert.equal(countOf("甲").lines, 1);
    });
});

describe("状态行那句话", () => {
    it("字符与字都摆出来", () => {
        assert.equal(describeCount(countOf("水文")), "2 字符 · 2 字");
    });
});