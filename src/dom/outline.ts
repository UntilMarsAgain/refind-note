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
 * 从**渲染好的正文**里数出那些标题。
 *
 * 这是 `dom/` 那一层该做的事（要碰 DOM），算出来的层级归 `core/outline.ts` 管。
 * 之所以读 DOM 而不是解析 markdown 源码，理由在 `core/outline.ts` 开头：
 * `id` 是渲染时给的，而正文里的标题也不止 `# 标题` 一种来路。
 */

import type { Heading } from "../core/outline.ts";

/** 六个一起查：`querySelectorAll` 按**文档次序**返回，不会先列完 h1 再列 h2 */
const SELECTOR = "h1, h2, h3, h4, h5, h6";

/**
 * 正文容器里的标题，按**出现次序**（也就是目录要显示的次序）。
 *
 * ## 要跳过的
 *
 * - **没有 `id` 的**：点不了就不该列。正则渲染出来的标题都有 id，
 *   但 `::html` 里作者手写的 `<h2>` 不一定有 —— 列出来就是个死条目。
 * - **代码块与模板块的标记**：那些只是长得像标题（`.code-frame` 里有行号列，
 *   模板块的头行会被编辑器标出来），它们不是这篇的章节。
 */
export function headingsIn(root: ParentNode | null): Heading[] {
    if (!root) {
        return [];
    }

    const found: Heading[] = [];
    for (const element of root.querySelectorAll(SELECTOR)) {
        if (!(element instanceof HTMLElement)) {
            continue;
        }
        if (!element.id || element.closest(".code-frame, .template-head")) {
            continue;
        }
        const tag = element.tagName.toLowerCase();
        found.push({
            id: element.id,
            level: Number(tag.slice(1)),
            text: element.textContent ?? "",
        });
    }
    return found;
}