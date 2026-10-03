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
 * 垂直标签栏（`components/shell/TabRail.vue`）的**交互那一层**：
 * 抖动、拖放换序、右键菜单。
 *
 * 与模板分开，是因为这三件事各自都是一段独立的时序，且都**不属于任何一个标签页**：
 *
 * - **抖动**是一次性的定时过程（收到信号 → 记住是哪一格 → 420ms 后摘掉），
 *   时长必须与 `rail.css` 里那条动画一致，两处各写各的很容易改一边忘一边；
 * - **拖放**要跨 `dragstart` / `dragover` / `drop` / `dragend` 四个事件保存
 *   "正在拖谁、拖到哪一格上面"，把它摊在模板里就是四个 handler 抢同一对 ref；
 * - **右键菜单**那一项（"重新打开关闭的标签页"）**看情况给不给**，
 *   给不给取决于外面有没有关掉过标签页 —— 那是标签页状态的事，不是标签栏的事。
 *
 * 所以这里收的都是**取值函数**（`() => props.x`）而不是值：状态只有一份，
 * 在 `core/tabs.ts` 与 `App.vue` 的 props 上，这里不留副本。
 * 真正"要做什么"则以 `on` 递进来 —— 标签页增删改序归 `core/tabs.ts` 管，
 * 标签栏只负责把用户的意图转过去。
 */

import { onBeforeUnmount, ref, watch, type Ref } from "vue";
import { openMenu } from "../dom/context-menu.ts";

/**
 * 抖动动画的时长（毫秒）。
 *
 * 与 `rail.css` 里 `.rail__item--shake` 的 `animation` 时长、以及
 * `prefers-reduced-motion` 那条 `rail-flash` 的时长是**同一个数** ——
 * 到点把类摘掉，下次才能再触发，所以定时器早于动画结束会看到"抖到一半停住"。
 */
const SHAKE_MS = 420;

/** 标签栏要往上报的那几件事：全都交给 `core/tabs.ts`，标签栏自己不动数据 */
export interface TabRailActions {
    close: (index: number) => void;
    closeOthers: (index: number) => void;
    /** 重新打开刚关掉的那个（快捷键是 Ctrl+Shift+T） */
    reopen: () => void;
    /** 拖放换序 */
    move: (from: number, to: number) => void;
}

export interface TabRailOptions {
    /** 当前选中的下标：抖动从这一格开始 */
    active: () => number;
    /** 有没有"刚关掉、可以重新打开"的标签页（决定右键菜单里给不给那一项） */
    canReopen: () => boolean;
    /** 抖动信号：每变一次就让标签动一下 */
    shakeTick: () => number;
    actions: TabRailActions;
}

export interface TabRail {
    /** 正在抖的是哪一格（连索引一起记下来：之后切标签不该把动画挪走） */
    shakeIndex: Ref<number | null>;
    /** 正在拖的标签页；`null` = 没在拖 */
    dragging: Ref<number | null>;
    /** 拖到了哪一格上面（用来画插入位置） */
    overIndex: Ref<number | null>;
    onDragStart: (event: DragEvent, index: number) => void;
    onDragOver: (event: DragEvent, index: number) => void;
    onDragEnd: () => void;
    onDrop: (index: number) => void;
    onTabMenu: (event: MouseEvent, index: number) => void;
}

export function useTabRail(options: TabRailOptions): TabRail {
    const shakeIndex = ref<number | null>(null);
    let shakeTimer: number | undefined;

    watch(
        () => options.shakeTick(),
        () => {
            shakeIndex.value = options.active();
            window.clearTimeout(shakeTimer);
            shakeTimer = window.setTimeout(() => {
                shakeIndex.value = null;
            }, SHAKE_MS);
        },
    );

    onBeforeUnmount(() => window.clearTimeout(shakeTimer));

    const dragging = ref<number | null>(null);
    const overIndex = ref<number | null>(null);

    function onDragStart(event: DragEvent, index: number) {
        dragging.value = index;
        // 不带数据的拖动在 WebKit 里根本不算"拖起来"（拖到一半就没有了），
        // 所以哪怕用不上也塞一份进去
        event.dataTransfer?.setData("text/plain", String(index));
        if (event.dataTransfer) {
            event.dataTransfer.effectAllowed = "move";
        }
    }

    function onDragOver(event: DragEvent, index: number) {
        overIndex.value = index;
        // 光标显示成"移动"，而不是"复制"
        if (event.dataTransfer) {
            event.dataTransfer.dropEffect = "move";
        }
    }

    function onDragEnd() {
        dragging.value = null;
        overIndex.value = null;
    }

    function onDrop(index: number) {
        const from = dragging.value;
        dragging.value = null;
        overIndex.value = null;
        if (from !== null && from !== index) {
            options.actions.move(from, index);
        }
    }

    /**
     * 标签页上的右键：关闭 / 关闭其它。
     *
     * 中键关闭是浏览器时代的习惯（模板里已经有了），右键这两项是给"一口气关掉一堆"用的。
     */
    function onTabMenu(event: MouseEvent, index: number) {
        event.preventDefault();
        event.stopPropagation();
        const items = [
            { label: "关闭标签页", run: () => options.actions.close(index) },
            { label: "关闭其它标签页", run: () => options.actions.closeOthers(index) },
        ];
        // 没有关掉过标签页时不给这一项：给了也没得开
        if (options.canReopen()) {
            items.push({ label: "重新打开关闭的标签页", run: () => options.actions.reopen() });
        }
        openMenu(event, items);
    }

    return {
        shakeIndex,
        dragging,
        overIndex,
        onDragStart,
        onDragOver,
        onDragEnd,
        onDrop,
        onTabMenu,
    };
}
