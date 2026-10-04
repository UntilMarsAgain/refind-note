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

import { onScopeDispose, ref, watch, type Ref } from "vue";
import { activeOf, outlineOf, shouldShowOutline, type OutlineEntry } from "../core/outline.ts";
import { headingsIn } from "../dom/outline.ts";

/**
 * 视口上沿往下这么多像素算"读过了"。
 *
 * 要**大于**页头那条细栏（收起后约 30 多像素）：阈值比它小的话，
 * 标题刚被页头盖住它就灭了 —— 于是"正读着的那一节"在开头几行里永远亮不出来。
 */
const READ_PAST = 96;

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
    /** 用户点的位置**优先于**滚动算出来的：点了就是点了，别马上被滚动覆盖掉 */
    let pinned = "";

    /** 重新数一遍标题 */
    function refresh() {
        const found = outlineOf(headingsIn(content.value?.rootEl ?? null));
        // 不够格就不给（见 `core/outline.ts` 的 `MIN_HEADINGS`）
        entries.value = shouldShowOutline(found) ? found : [];
        // 换了一篇正文，之前那一条钉住的自然作废
        pinned = "";
        follow();
    }

    /** 按"眼下读到哪儿了"点亮那一条 */
    function follow() {
        if (pinned) {
            return;
        }
        const scroller = scrollerOf(content.value?.rootEl ?? null);
        const root = content.value?.rootEl ?? null;
        if (!scroller || !root) {
            return;
        }
        const origin = scroller.getBoundingClientRect().top;
        const tops = entries.value.map((entry) => {
            const element = root.querySelector(`#${CSS.escape(entry.id)}`);
            return element
                ? element.getBoundingClientRect().top - origin
                : Number.POSITIVE_INFINITY;
        });
        const at = activeOf(tops, READ_PAST);
        active.value = at < 0 ? "" : (entries.value[at]?.id ?? "");
    }

    /** 滚动容器是正文最近的那个可滚祖先（渲染区那个 `.pane`） */
    function scrollerOf(element: HTMLElement | null): HTMLElement | null {
        for (let node = element?.parentElement ?? null; node; node = node.parentElement) {
            // `overflow` 的四个值里只要有一个不是 `visible`，它就可能是滚动容器
            if (/(auto|scroll|overlay)/.test(getComputedStyle(node).overflowY)) {
                return node;
            }
        }
        return null;
    }

    // 滚动很密（拖滑块、惯性），所以攒到下一帧再算一次
    let ticking = false;
    function onScroll() {
        if (ticking) {
            return;
        }
        ticking = true;
        requestAnimationFrame(() => {
            ticking = false;
            follow();
        });
    }

    let listening: HTMLElement | null = null;
    function listen() {
        if (listening) {
            return;
        }
        listening = scrollerOf(content.value?.rootEl ?? null);
        listening?.addEventListener("scroll", onScroll, { passive: true });
    }

    // 组件停用（KeepAlive）与卸载时都要摘掉 —— 否则标签页换得多了，
    // 上一次的监听还挂着，白算一长串（这正是 `RenderPane` 那条长注释讲的那类事）
    onScopeDispose(() => listening?.removeEventListener("scroll", onScroll));

    // 容器换了一个（`NoteContent` 是 `v-else-if` 分支，正文换了组件也会换）
    watch(content, () => {
        // 换容器就换滚动容器：先把旧的摘掉，再找新的
        listening?.removeEventListener("scroll", onScroll);
        listening = null;
        refresh();
        listen();
    }, { immediate: true });

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
        // 点过就一直亮着：接下来那一下滚动事件会立刻按位置算，
        // 而 `scrollIntoView` 到那一步的位置可能还没算准 —— 不钉住的话，
        // 点一下刚亮起来的条目会立刻灭掉（"点了没反应"的另一种形态）
        pinned = id;
        active.value = id;
    }

    return {
        entries,
        active,
        pick,
        // 正文里点锚点：那是"用户去的地方"，同样钉住
        mark: (id: string) => {
            pinned = id;
            active.value = id;
        },
        // 地址里带来的章节（深链 / 后退回来）：亮它，但**不钉** ——
        // 那是"我落在这一节"，往下读就该跟着读的位置走
        sync: (id: string) => {
            active.value = id;
        },
    };
}