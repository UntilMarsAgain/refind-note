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
