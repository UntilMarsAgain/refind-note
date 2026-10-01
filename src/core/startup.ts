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

export const startupPhase = readonly(phase);
export const startupFailure = readonly(failure);

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
