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
 * Ctrl + 滚轮的全局缩放。
 *
 * 与设置页里那个缩放是**同一个值**：这里即时生效，落盘交给 `preferences.ts` 的节流。
 */

import { preferences, updatePreferences } from "../core/preferences.ts";

/** 与设置页的输入范围一致 */
const ZOOM_MIN = 0.5;
const ZOOM_MAX = 3;

/** 一格滚轮 10% */
const ZOOM_STEP = 0.1;

export function installWheelZoom(): void {
    window.addEventListener("wheel", onWheel, { passive: false });
}

function onWheel(event: WheelEvent): void {
    if (!event.ctrlKey) {
        return;
    }

    // 拦掉 WebView 自己的 Ctrl+滚轮行为，免得两套缩放打架
    event.preventDefault();

    const current = preferences.value.zoom;
    const stepped = current - Math.sign(event.deltaY) * ZOOM_STEP;
    // 定到两位小数：一格一格加减会攒出 0.30000000000000004 这种尾数
    const next = Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, Math.round(stepped * 100) / 100));

    if (next !== current) {
        updatePreferences({ zoom: next });
    }
}
