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
 * 本页目录：把一篇正文里的标题收成一份可点的清单。
 *
 * ## 为什么是"读渲染结果"而不是"解析 markdown"
 *
 * 标题的 `id` 是**渲染时**生成的（后端 `markdown/mod.rs` 的 `slugify_heading`），
 * 而正文里能出现的标题不止 `# 标题` 一种写法 —— `::html` 里手写的 `<h2>`、
 * `<markdown>` 嵌进来的片段、模板套模板，全都会变成标题。
 * 于是"目录该列哪几条"的唯一真相就是**渲染之后那棵树**：数一遍 `<h1>`…`<h6>` 就行。
 *
 * 代价是目录要等正文渲染完才知道（那是 DOM 的事，见 `dom/outline.ts`）；
 * 而这一份（怎么把一份平的标题列表变成可点的层级）是纯计算，所以放在 `core/`。
 *
 * ## 为什么不直接用浏览器的目录
 *
 * `<details>` + 锚点是能用的，但两处不对：标题重名时 `id` 会带后缀，
 * 而 `markdown-it` 生成的 `href` 是**百分号编码**过的中文，与 `id` 对不上；
 * 另外目录项要能报出"点了哪一节"（好让地址栏跟着变），浏览器那份报不出来。
 */

/** 一条标题 */
export interface Heading {
    /** 锚点：正文里那个元素的 `id`（点它就滚过去） */
    id: string;
    /** 1…6（`h1`…`h6`） */
    level: number;
    /** 显示的文字 */
    text: string;
}

/** 目录里的一项：在标题之上多了"缩进几层" */
export interface OutlineEntry extends Heading {
    /**
     * 缩进层数（0 起）。
     *
     * **不是 `level - 1`**：作者可以从 `#` 直接跳到 `####`（那不是笔误，
     * 深层笔记里很常见），照 `level` 缩进会让目录右边空出三格。
     * 这里按**实际出现过的那几级**重新编号，于是目录永远是紧凑的一列。
     */
    depth: number;
}

/**
 * 少于这么多标题就不给目录。
 *
 * 三个标题以内，那份清单比正文还长，而正文就在它下面 —— 让人多点一次才看得见。
 * 四开始才真的能当"目录"用。
 */
export const MIN_HEADINGS = 4;

/** 摘掉标题里那些不该显示出来的标记 */
function plain(text: string): string {
    return text.replace(/\s+/g, " ").trim();
}

/**
 * 把一份平的标题列表变成目录。
 *
 * 只做两件事：**丢掉空的**（`##` 后面什么都不写的那种，渲染出来是个空标题，
 * 列在目录里是个点不开的条目）与**按实际用到的层级压紧缩进**。
 * 顺序原样保留 —— 目录的次序就是正文的次序，不另按字母排。
 */
export function outlineOf(headings: readonly Heading[]): OutlineEntry[] {
    const kept = headings.filter((heading) => heading.id && plain(heading.text));
    if (kept.length === 0) {
        return [];
    }

    // 这一篇里实际用到的那几级，从浅到深压成 0、1、2…
    const levels = [...new Set(kept.map((heading) => heading.level))].sort((a, b) => a - b);

    return kept.map((heading) => ({
        id: heading.id,
        level: heading.level,
        text: plain(heading.text),
        depth: Math.max(0, levels.indexOf(heading.level)),
    }));
}

/** 这一篇值不值得摆一个目录 */
export function shouldShowOutline(entries: readonly OutlineEntry[]): boolean {
    return entries.length >= MIN_HEADINGS;
}