/**
 * 启动：进行到哪一步了。
 *
 * 启动是一串步骤：打开工作目录 → 认数据库 → 读偏好。任何一步失败都报到这里。
 *
 * 三种状态对应主区域的三种样子（见 `App.vue`）：还在跑给加载页、跑完给正常界面、
 * 没跑完给错误页。失败**不退出**：窗口照常开、标题栏照常能用（至少关得掉）——
 * 启动失败最怕的是"什么都没发生"。
 */

import { readonly, ref } from "vue";

export interface StartupFailure {
    /** 哪一步失败的 */
    step: string;
    /** 具体原因 */
    detail: string;
}

export type StartupPhase = "starting" | "ready" | "failed";

const phase = ref<StartupPhase>("starting");
const failure = ref<StartupFailure | null>(null);

/**
 * 现在在做哪一步（加载页上那一行字）。
 *
 * 启动不是一瞬间的事：打开目录、读偏好、还要（开着的话）与云端同步一次。
 * 这行字让等待有内容 —— 尤其同步，它可能要跑十几秒，光转圈会让人以为卡死了。
 */
const note = ref("正在打开工作目录…");

export const startupPhase = readonly(phase);
export const startupFailure = readonly(failure);
export const startupNote = readonly(note);

export function setStartupNote(text: string): void {
    note.value = text;
}

export function markStartupReady(): void {
    phase.value = "ready";
    failure.value = null;
}

export function reportStartupFailure(step: string, error: unknown): void {
    phase.value = "failed";
    failure.value = {
        step,
        detail: error instanceof Error ? error.message : String(error),
    };
    console.error(`启动失败（${step}）：`, error);
}

/** 重试：回到"正在启动"，加载页会重新出现 */
export function restartStartup(): void {
    phase.value = "starting";
    failure.value = null;
}
