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
 * `core/markdown/template-blocks.ts`：模板块的边界规则。
 *
 * 这些函数是**后端扫描器的镜像**（见文件头），两边对不上时高亮就会说谎，
 * 而那种错在界面上表现为"块在这里断了"，极难看出来 —— 所以规矩要钉住。
 */

import assert from "node:assert/strict";
import { describe, it } from "node:test";

import {
    headIndent,
    isBlank,
    isTemplateHead,
    scriptRanges,
    templateBlockEnd,
    templateBlockLines,
    templateFoldRange,
    templateMarks,
    templateNameOf,
    templateRanges,
} from "../../src/core/markdown/template-blocks.ts";

describe("行首空白与空行", () => {
    it("数出的是空白字符个数（制表符算一个）", () => {
        assert.equal(headIndent("    缩进"), 4);
        assert.equal(headIndent("\t制表"), 1);
        assert.equal(headIndent("没有"), 0);
    });

    it("只有空白也算空行", () => {
        assert.equal(isBlank(""), true);
        assert.equal(isBlank("   \t "), true);
        assert.equal(isBlank(" 有 "), false);
    });
});

describe("头行的认法", () => {
    it("认得普通的头行", () => {
        assert.equal(isTemplateHead("::note"), true);
        assert.equal(isTemplateHead("  ::note"), true);
        assert.equal(isTemplateHead("::note title=\"含 空格\""), true);
    });

    it("不是头行的几种", () => {
        assert.equal(isTemplateHead(""), false);
        assert.equal(isTemplateHead("note"), false);
        assert.equal(isTemplateHead(":::note"), false); // 只有一个名字
        assert.equal(isTemplateHead("::"), false);
    });

    it("名字里带 `=` 或 `:` 的退回普通段落（与后端同一口径）", () => {
        assert.equal(isTemplateHead("::a=b"), false);
        assert.equal(isTemplateHead("::a:b"), false);
        assert.equal(isTemplateHead("::note=x"), false);
    });

    it("引号包住的名字可以带空格，但引号要配上", () => {
        assert.equal(isTemplateHead('::"两个 词"'), true);
        assert.equal(isTemplateHead('::"没关上'), false);
    });

    it("取名字", () => {
        assert.equal(templateNameOf("::note"), "note");
        assert.equal(templateNameOf("::code lang=js"), "code");
        assert.equal(templateNameOf('::"两个 词" key=1'), "两个 词");
        assert.equal(templateNameOf("不是头行"), "");
        assert.equal(templateNameOf("::a:b"), "");
    });
});

describe("块边界由缩进划出", () => {
    it("缩进更深的行属于块内，遇到不更深的就结束", () => {
        const lines = ["::note", "  第一行", "  第二行", "块外了"];
        assert.equal(templateBlockEnd(lines, 0, 0), 2);
    });

    it("一条都不属于时返回头行自己（表示空块）", () => {
        assert.equal(templateBlockEnd(["::note", "块外"], 0, 0), 0);
    });

    it("空行不直接结束块 —— 后面还有更深的内容就收进来", () => {
        const lines = ["::note", "  第一段", "", "  第二段还在块里", "", "这段已经在块外了"];
        // 中间那个空行（2）归块内；块尾紧跟着的空行（4）归块外 ——
        // 判据是"往后看一行"，而往后看已经是不更深的了
        assert.equal(templateBlockEnd(lines, 0, 0), 3);
    });

    it("块尾的空行归块外（它后面没有更深的内容了）", () => {
        const lines = ["::note", "  正文", "", "外面"];
        assert.equal(templateBlockEnd(lines, 0, 0), 1);
    });

    it("连续几个空行后跟不更深的内容，空行就归块外", () => {
        const lines = ["::note", "  正文", "", "   ", "外面"];
        assert.equal(templateBlockEnd(lines, 0, 0), 1);
    });

    it("头行自己缩进时按它的缩进算（列表里的块不会被外面吃掉）", () => {
        const lines = ["- 列表项", "  ::note", "    块内", "  块外"];
        assert.equal(templateBlockEnd(lines, 1, 2), 2);
    });
});

describe("覆盖的行", () => {
    it("块覆盖头行与块内全部行，空行也算（块的左边缘要连着画）", () => {
        const lines = ["::note", "  第一段", "", "  第二段", "外面"];
        assert.deepEqual(templateBlockLines(lines), [0, 1, 2, 3]);
    });

    it("折叠范围是头行之后的那一段，空块没有折叠箭头", () => {
        const lines = ["::note", "  内容", "外面"];
        assert.deepEqual(templateFoldRange(lines, 0), { from: 1, to: 1 });
        assert.equal(templateFoldRange(["::note", "外面"], 0), null);
        assert.equal(templateFoldRange(["不是头行"], 0), null);
    });
});

describe("装饰", () => {
    it("头行整行标出，块内只标行首缩进", () => {
        const lines = ["::note", "  内容很长的一行"];
        assert.deepEqual(templateMarks(lines), [
            { line: 0, head: true, start: 0, end: 6 },
            { line: 1, head: false, start: 0, end: 2 },
        ]);
    });

    it("带空白的空行不标（否则看着像块断了）", () => {
        const marks = templateMarks(["::note", "   ", "  内容"]);
        assert.deepEqual(
            marks.filter((mark) => mark.line === 1),
            [],
        );
    });

    it("嵌套的头行只算自己的头行，不算外层的块内", () => {
        const lines = ["::note", "  ::quote", "    引用"];
        const marks = templateMarks(lines);
        assert.deepEqual(marks.filter((mark) => mark.line === 1), [
            { line: 1, head: true, start: 0, end: 9 },
        ]);
        assert.ok(marks.some((mark) => mark.line === 2 && mark.head === false));
    });

    it("换算成文档偏移量后与实际文字对得上", () => {
        const lines = ["::note", "  甲"];
        const ranges = templateRanges(lines);
        // 第一行 0..6，第二行从 7 开始（换行算一个字符）
        assert.deepEqual(ranges, [
            { from: 0, to: 6, head: true },
            { from: 7, to: 9, head: false },
        ]);
    });
});

describe("按 JavaScript 上色的那几块", () => {
    it("`::js` 的块内算一段", () => {
        const lines = ["::js", "  const a = 1;", "  console.log(a);", "外面"];
        const ranges = scriptRanges(lines);
        assert.equal(ranges.length, 1);
        assert.equal(ranges[0]?.code, "  const a = 1;\n  console.log(a);");
    });

    it("`::code lang=js` 也算（显示成代码就该有颜色）", () => {
        assert.equal(scriptRanges(['::code lang="JS"', "  f();"]).length, 1);
        assert.equal(scriptRanges(["::code lang=javascript", "  f();"]).length, 1);
    });

    it("别的语言不算", () => {
        assert.deepEqual(scriptRanges(["::code lang=rust", "  fn f() {}"]), []);
        assert.deepEqual(scriptRanges(["::code", "  f();"]), []);
    });

    it("没有块内内容的头行不算（空块没什么可上色）", () => {
        assert.deepEqual(scriptRanges(["::js", "外面"]), []);
    });

    it("偏移量正好是块内那段（不含头行，也不含外面那一行）", () => {
        const lines = ["::js", "  a();", "外面"];
        const [range] = scriptRanges(lines);
        const text = lines.join("\n");
        assert.equal(text.slice(range?.from, range?.to), "  a();");
    });
});