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
 * 「本页目录」的接线：数标题、等正文收拾完、点一条滚过去。
 *
 * ## 为什么抽出来
 *
 * 阅读笔记（`NoteView`）与读帮助页（`HelpView`）是**两个组件、同一件事**：
 * 它们都渲染一段 HTML、都把它交给同一个 `NoteContent`、都需要一份目录。
 * 各自写一遍就要在两个地方各盯着"什么时候重数一次"（那是这件事里最容易错的一步），
 * 所以收在这里 —— 与 `useFileStorage` 那条"两页不各写一遍 `policy.symmetric ? …`"
 * 是同一个道理。
 *
 * 判定与层级在 `core/outline.ts`，从 DOM 里收标题在 `dom/outline.ts`。
 */

import { ref, watch, type Ref } from "vue";
import { outlineOf, shouldShowOutline, type OutlineEntry } from "../core/outline.ts";
import { headingsIn } from "../dom/outline.ts";

/** `NoteContent` 交出来的那一点：正文容器与"收拾好了"的计数 */
export interface ContentHandle {
    rootEl: HTMLElement | null;
    ready: number;
}

export interface Outline {
    /** 目录项（层级已算好）；条目不足四节时是空数组 */
    entries: Ref<OutlineEntry[]>;
    /** 当前所在的那一节 */
    active: Ref<string>;
    /** 点了目录里的一条：滚过去，并把章节报给上层叠进地址 */
    pick: (id: string) => void;
    /** 正文里点锚点：目录上那一项要跟着亮 */
    mark: (id: string) => void;
    /** 地址里带来的章节（深链 / 后退回来）：也要亮 */
    sync: (id: string) => void;
}

export function useOutline(content: Ref<ContentHandle | null>): Outline {
    const entries = ref<OutlineEntry[]>([]);
    const active = ref("");

    /** 重新数一遍标题 */
    function refresh() {
        const found = outlineOf(headingsIn(content.value?.rootEl ?? null));
        // 不够格就不给（见 `core/outline.ts` 的 `MIN_HEADINGS`）
        entries.value = shouldShowOutline(found) ? found : [];
    }

    // 容器换了一个（`NoteContent` 是 `v-else-if` 分支，正文换了组件也会换）
    watch(content, () => refresh(), { immediate: true });

    // **正文重渲染**也要重数：`v-html` 换一次内容，组件与容器都还是那一个，
    // 标题却已经是新的一批。`ready` 是 `NoteContent` 收拾完之后才递增的计数 ——
    // 等它再数，才不会数到一半的 DOM。
    watch(
        () => content.value?.ready ?? 0,
        () => refresh(),
    );

    function pick(id: string) {
        // `CSS.escape`：锚点里可能有中文与连字符，直接拼进选择器会抛
        content.value?.rootEl?.querySelector(`#${CSS.escape(id)}`)?.scrollIntoView({
            block: "start",
        });
        active.value = id;
    }

    return { entries, active, pick, mark: (id) => (active.value = id), sync: (id) => (active.value = id) };
}