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
 * `core/title.ts` 与 `core/editor-state.ts`：两个小而要紧的纯逻辑。
 *
 * 前者是"标题的字面处理"，后者是"每个标签页自己的编辑状态"——
 * 后者尤其要盯住**按标签页 id 分开**这件事：串了就是"切回标签页看到别的页的字"，
 * 而那种错在界面上表现为"偶尔记错"，几乎不可能靠肉眼发现。
 */

import assert from "node:assert/strict";
import { describe, it } from "node:test";

import { forget, recall, remember } from "../../src/core/editor-state.ts";
import { initialOf, parentOf } from "../../src/core/title.ts";

describe("列表里那个单字图标", () => {
    it("取名称段的首字，不取命名空间前缀", () => {
        assert.equal(initialOf("special:debug"), "d");
        assert.equal(initialOf("File:桥.png"), "桥");
    });

    it("没有冒号就是整个标题的首字", () => {
        assert.equal(initialOf("水文"), "水");
    });

    it("多字节字符与代理对不被切成半个", () => {
        // 这个 emoji 是代理对（两个 UTF-16 码元），`[0]` 只会拿到一半
        assert.equal(initialOf("🧭罗盘"), "🧭");
    });

    it("空标题给个占位，不给空串（空串会让图标整个消失）", () => {
        assert.equal(initialOf(""), "•");
        assert.equal(initialOf("   "), "•");
    });
});

describe("父页面", () => {
    it("按最后一个斜杠切，只上去一层", () => {
        assert.equal(parentOf("Test/航道/水文"), "Test/航道");
        assert.equal(parentOf("Test/航道"), "Test");
    });

    it("开头的斜杠不算父级（那是子页，没有独立的名字可回）", () => {
        assert.equal(parentOf("/航道"), "");
    });

    it("没有斜杠就没有父级", () => {
        assert.equal(parentOf("水文"), "");
    });
});

describe("每个标签页各自的编辑状态", () => {
    const state = (markdown: string) => ({ markdown, cursor: 3, scroll: 120 });

    it("记下就能原样取回（字、光标、滚动）", () => {
        remember("tab-1", state("甲"));
        assert.deepEqual(recall("tab-1"), state("甲"));
        forget("tab-1");
    });

    it("两个标签页各记各的，不串", () => {
        remember("tab-a", state("甲的正文"));
        remember("tab-b", state("乙的正文"));
        assert.equal(recall("tab-a")?.markdown, "甲的正文");
        assert.equal(recall("tab-b")?.markdown, "乙的正文");
        forget("tab-a");
        forget("tab-b");
    });

    it("没在编辑过的标签页给 undefined（不是空状态）", () => {
        // 区别很要紧：空状态会被当成"有一个空编辑器"而跳过"恢复草稿"那个提示
        assert.equal(recall("从没见过"), undefined);
    });

    it("没有标签页 id 时也给 undefined（而不是去问一个空串）", () => {
        assert.equal(recall(undefined), undefined);
    });

    it("忘掉一个不影响另一个", () => {
        remember("tab-c", state("丙"));
        remember("tab-d", state("丁"));
        forget("tab-c");
        assert.equal(recall("tab-c"), undefined);
        assert.equal(recall("tab-d")?.markdown, "丁");
        forget("tab-d");
    });
});