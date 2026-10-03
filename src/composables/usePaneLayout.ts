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
 * 编辑器那两栏（源码 / 预览）的**排布测量**：并排还是上下排列。
 *
 * 为什么单独一个模块：它是一段自成一体的浏览器测量逻辑 —— 量容器宽度、跨过阈值就换方向、
 * 宽度变了补测一次 —— 与笔记内容、与 CodeMirror 都毫无关系。抽出来之后组件里只剩
 * 「按 `stacked` 把两栏摆成 row 还是 column」这一句，迟滞判定与观察器都归这里；
 * 改阈值不必翻组件。
 *
 * 为什么放在 `composables/` 而不是 `dom/`：它要的是一个**模板 ref** 与组件的挂载时机，
 * 观察器与 resize 监听都随组件卸载而销毁。`dom/` 里那些模块是「对既有 DOM 打副作用」，
 * 不带 Vue 生命周期这一层。
 *
 * 尺寸常量也一并住在这里 —— 模板里内联的那几个 `flex` / `height` / `maxHeight`
 * 用的就是它们，测量与摆布必须对着同一组数字。
 */

import { onBeforeUnmount, onMounted, ref, type Ref } from "vue";

/**
 * 每栏的最小可读宽度。
 *
 * 低于这个宽度就**上下排列**，而不是硬挤成两条窄栏：一行代码在 260px 里要折好几次，
 * 折过之后比上下排列还难读。300 是"一行十几字符仍能看清"的位置。
 */
export const PANE_MIN_WIDTH = 300;
export const PANES_GAP = 12;
export const STACK_BREAKPOINT = PANE_MIN_WIDTH * 2 + PANES_GAP;
/**
 * 切回来的阈值比切过去的**高一点**（迟滞）。
 *
 * 两个值贴在一起时，一次布局变化（比如竖滚动条出现，占掉十几像素）就能让宽度在阈值两侧
 * 来回跳，于是"偶尔莫名其妙变成上下排布"。留出这段差量，来回都需要真正跨过一段距离。
 */
export const UNSTACK_BREAKPOINT = STACK_BREAKPOINT + 40;

/** 并排时单栏的高度上限（视口减去本页固定开销的权宜值） */
export const PANE_HEIGHT = "calc(100vh - 240px)";
/** 上下排布时每栏的高度：两者相加仍不超过上面那个值，页面不会被撑长 */
export const STACKED_PANE_HEIGHT = "calc((100vh - 240px) / 2)";

export interface PaneLayout {
    /**
     * 两栏容器：接在那一个 `<div class="editor__panes">` 上（函数 ref，见
     * `components/note/EditorPanes.vue` 的 `setPanesEl`）。
     *
     * 量**容器自己**而不是量两栏里的任何一栏 —— 理由见 `measurePanes` 里那段。
     */
    panesEl: Ref<HTMLElement | null>;
    /** 现在是上下排布（窄到一行代码会折得没法读） */
    stacked: Ref<boolean>;
}

/**
 * 在组件里用一次：把 `panesEl` 接到两栏容器上，就得到 `stacked`。
 *
 * 观察器与 resize 监听由这里自己装、自己卸 —— 组件不用记得清理，卸载时钩子已经跑过。
 */
export function usePaneLayout(): PaneLayout {
    const panesEl = ref<HTMLElement | null>(null);
    const stacked = ref(false);

    let panesObserver: ResizeObserver | undefined;

    function measurePanes() {
        const el = panesEl.value;
        if (!el) {
            return;
        }
        // 量两栏容器自己是对的：它宽度由父级决定（块级 flex 撑满），**不随排布方向变化**，
        // 所以不会出现"一变成上下排布、可用宽度也跟着变小，于是再也切不回来"的自反馈。
        const width = el.clientWidth;
        // 宽度为 0（还没布局 / 不可见）时不下结论，免得一上来就误判
        if (width <= 0) {
            return;
        }
        stacked.value = stacked.value
            ? width < UNSTACK_BREAKPOINT
            : width < STACK_BREAKPOINT;
    }

    onMounted(() => {
        measurePanes();
        // 挂载那一刻的宽度未必是最终宽度（滚动条、版心过渡、窗口管理器的初始摆放都可能插一脚），
        // 所以下一帧再量一次：迟滞判定只在真正跨过阈值时才改变结论，重测是安全的。
        requestAnimationFrame(measurePanes);

        if (typeof ResizeObserver !== "undefined" && panesEl.value) {
            panesObserver = new ResizeObserver(measurePanes);
            panesObserver.observe(panesEl.value);
        }
        // 窗口变化一律补测一次，不只在没有 ResizeObserver 时
        window.addEventListener("resize", measurePanes);
    });

    onBeforeUnmount(() => {
        panesObserver?.disconnect();
        panesObserver = undefined;
        window.removeEventListener("resize", measurePanes);
    });

    return { panesEl, stacked };
}