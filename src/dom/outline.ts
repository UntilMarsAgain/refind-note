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
 *
 * ## 重名的标题：`id` 会重复，而那不是这里能修的
 *
 * 后端 `markdown/mod.rs` 那个 `slugify_heading` 是**纯函数**，而
 * `heading_anchors` 直接拿它的返回值当 `id` —— 所以三个 `## 表格` 渲染出来
 * 是三个 `id="表格"`，**并没有**错开（这一点由后端那条
 * `repeated_headings_get_distinct_anchors` 的邻居用例如实记着）。
 *
 * 所以错开在这一层做，而且只对**目录**生效：给第 n 个同名的编一个 `key`
 * （`表格~2`），定位时按「第几个同名的」去找元素。渲染结果一个字不动 ——
 * 正文里 `[文字](#表格)` 指向第一个，那是既有行为，不该被目录顺手改掉。
 */
export function headingsIn(root: ParentNode | null): Heading[] {
    if (!root) {
        return [];
    }

    const seen = new Map<string, number>();
    const found: Heading[] = [];
    for (const element of root.querySelectorAll(SELECTOR)) {
        if (!(element instanceof HTMLElement)) {
            continue;
        }
        if (!element.id || element.closest(".code-frame, .template-head")) {
            continue;
        }
        // 同名计数：第一个是 1，第二个 2……
        const occurrence = (seen.get(element.id) ?? 0) + 1;
        seen.set(element.id, occurrence);
        found.push({
            id: element.id,
            key: occurrence === 1 ? element.id : `${element.id}~${occurrence}`,
            occurrence,
            level: Number(element.tagName.slice(1).toLowerCase()),
            text: element.textContent ?? "",
        });
    }
    return found;
}