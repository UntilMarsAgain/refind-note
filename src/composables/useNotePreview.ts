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
 * 编辑器的**预览产出**：拿到 html，以及决定什么时候去渲染。
 *
 * 与别的部分分开，是因为"预览什么时候更新"是**编辑器特有的一套时序**：
 * 预览不追着输入跑（见 `PREVIEW_IDLE_MS`），攒到静默才往返一次后端。
 * 内容变了的一方（`useNoteEditing` 的 `markdown`）与摆预览那一层 DOM 的一方
 * （`components/note/EditorPanes.vue`）都不该管这件事。
 *
 * 预览由**后端**渲染，与阅读视图同一个渲染器 —— 所以预览里的表格、内部链接、
 * 代码高亮与正文逐字一致，不会出现"预览好看、提交后变样"。前端这边只负责拿到 html；
 * 公式（`dom/math.ts`）与图表（`dom/diagrams.ts`）的绘制都在注入之后，由
 * `dom/note-html.ts` 统一收尾（阅读视图与预览共用同一处），这里**不重复实现**。
 */

import { invoke } from "@tauri-apps/api/core";
import { onBeforeUnmount, ref, type Ref } from "vue";

/**
 * 预览**不追着输入跑**：连续 5 秒没有输入才渲染一次。
 *
 * 渲染要往返后端（而且是完整 markdown 渲染），逐字触发既费也可能打断思路；
 * 想看当前内容时按「刷新预览」立刻渲染。
 */
export const PREVIEW_IDLE_MS = 5000;

export interface NotePreview {
    /** 预览 html（由后端渲染，直接注入预览那一层） */
    preview: Ref<string>;
    /** 预览渲染失败的原因 */
    previewProblem: Ref<string>;
    /** 立刻渲染一次（不等静默）：编辑器打开时先给一屏 */
    refresh: (text: string) => Promise<void>;
    /** 内容变了：静默 `PREVIEW_IDLE_MS` 之后再渲染 */
    schedule: (text: string) => void;
    /** 「刷新预览」按钮：不等静默，立刻渲染**当前**内容 */
    refreshNow: () => void;
}

/**
 * 在组件里用一次。
 *
 * `titleOf` 与 `textOf` 都是取值的**函数**：预览的参数（渲染哪一篇、渲染什么内容）
 * 归编辑状态所有，这里只"问一句要"，不留副本 —— 留一份就多一处会过期的地方。
 */
export function useNotePreview(titleOf: () => string, textOf: () => string): NotePreview {
    const preview = ref("");
    const previewProblem = ref("");

    /** 预览渲染的定时器：连续输入期间只排最后一次 */
    let previewTimer: number | undefined;

    async function refresh(text: string) {
        try {
            preview.value = await invoke<string>("render_markdown", {
                markdown: text,
                title: titleOf(),
            });
        } catch (error) {
            preview.value = "";
            previewProblem.value = String(error);
        }
    }

    function schedule(text: string) {
        window.clearTimeout(previewTimer);
        previewTimer = window.setTimeout(() => void refresh(text), PREVIEW_IDLE_MS);
    }

    /** 手动刷新预览（不等静默） */
    function refreshNow() {
        window.clearTimeout(previewTimer);
        void refresh(textOf());
    }

    onBeforeUnmount(() => {
        window.clearTimeout(previewTimer);
        previewTimer = undefined;
    });

    return { preview, previewProblem, refresh, schedule, refreshNow };
}