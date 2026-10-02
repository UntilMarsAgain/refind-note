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
 * 一条挂在顶部的短提示。
 *
 * 有一批按钮是**假的**，点它们不该毫无反应 —— 毫无反应的按钮会被当成"坏了"。
 * 所以假按钮统一走这里说一句实话，提示自己会散掉，不需要用户去关。
 */

import { ref } from "vue";

/** 当前提示；空串表示没有 */
export const notice = ref("");

/** 默认停留时长 */
const DEFAULT_MS = 2600;

let timer: number | undefined;

/** 说一句，过一会儿自己散掉；连着说时以后一句为准 */
export function flash(text: string, ms = DEFAULT_MS): void {
    notice.value = text;
    window.clearTimeout(timer);
    timer = window.setTimeout(() => {
        notice.value = "";
    }, ms);
}

export function dismissNotice(): void {
    window.clearTimeout(timer);
    notice.value = "";
}
