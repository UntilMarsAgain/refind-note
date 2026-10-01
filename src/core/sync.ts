/**
 * 同步的**时机**都收在这一处。
 *
 * 什么在什么时候同步，是这个模块唯一要回答的问题：
 *
 * - **启动时**：`syncAtStartup`，排在打开数据库之前（理由见 `preferences.ts`）；
 * - **提交之后**：`requestSyncAfterCommit`，攒一会儿再跑 —— 连着提交几次只同步一次；
 * - **手动**：`syncNow`，设置页那颗按钮；
 * - **关窗之前**：`syncBeforeClose`，把这一趟的改动送出去再走。
 *
 * 草稿**不参与**：它是"写了一半的本机缓冲"，每篇一个槽位、随时会被覆盖 ——
 * 传上去只会让两台机器互相盖。真正的内容以**提交**为准（这一条不是这里决定的，
 * 是工作目录那一层把 `db/drafts/` 点名排除了）。
 *
 * 引擎（谁新谁旧、怎么加密）全在后端，这里只决定"什么时候叫它"。
 */

import { readonly, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { SyncProgress, SyncReport } from "../ipc/sync.ts";
import { flash } from "./notice.ts";
import { setStartupNote } from "./startup.ts";

/** 正在同步（同一时刻只跑一次） */
const busy = ref(false);
/** 跑到哪儿了（进度条/加载页用） */
const progress = ref<SyncProgress | null>(null);
/** 关窗之前那一次：界面据此摆一块遮罩，别让人对着没反应的窗口再点一次 */
const closing = ref(false);

export const syncBusy = readonly(busy);
export const syncProgress = readonly(progress);
export const syncClosing = readonly(closing);

/** 提交之后攒多久再同步：连着提交几次只跑一次 */
const AFTER_COMMIT_DELAY_MS = 20_000;

/** 排队中的定时器与"跑完还要不要再跑一次" */
let timer: number | undefined;
let queued = false;

/** 同步开着吗、配置填全了吗（后端说了算） */
export async function syncReady(): Promise<boolean> {
    try {
        return await invoke<boolean>("sync_ready");
    } catch (error) {
        console.warn("问同步状态失败：", error);
        return false;
    }
}

/**
 * 跑一次。
 *
 * 同一时刻只有一趟：正在跑的时候再叫，就在跑完之后**再补一趟**（不是丢掉——
 * 那期间可能正好提交了新东西）。
 */
async function run(): Promise<SyncReport | null> {
    if (busy.value) {
        queued = true;
        return null;
    }
    busy.value = true;
    progress.value = null;

    const unlisten = await listen<SyncProgress>("sync-progress", (event) => {
        progress.value = event.payload;
    });

    try {
        return await invoke<SyncReport>("sync_now");
    } catch (error) {
        // 同步失败不是"操作失败"：先说清，再让人该干嘛干嘛
        flash(`同步没成功：${error}`);
        return null;
    } finally {
        unlisten();
        busy.value = false;
        progress.value = null;
        if (queued) {
            queued = false;
            void run();
        }
    }
}

/**
 * 现在同步一次（设置页那颗按钮）。
 *
 * 刚下载回来的东西，**当前打开着的页面看不见** —— 那几页还拿着旧内容。
 * 所以取回来之后说一句，让人知道该把页面重新打开。
 */
export async function syncNow(): Promise<SyncReport | null> {
    const report = await run();
    if (report && report.downloaded > 0) {
        flash(`取回 ${report.downloaded} 份；重新打开标签页就能看到`);
    }
    return report;
}

/**
 * 提交之后叫一次：攒一会儿再跑，免得连着提交几次就同步几次。
 *
 * 没开同步就**什么也不做**（也不要去问后端一遍又一遍）—— 提交本身与同步无关。
 */
export function requestSyncAfterCommit(): void {
    window.clearTimeout(timer);
    timer = window.setTimeout(async () => {
        if (await syncReady()) {
            void run();
        }
    }, AFTER_COMMIT_DELAY_MS);
}

/**
 * 启动时同步一次（开着的话）——**要在打开数据库之前**跑，理由见 `preferences.ts`。
 *
 * 失败不挡启动：网断了、桶名写错了、锁被别人拿着，都不该让人打不开自己的笔记。
 */
export async function syncAtStartup(): Promise<void> {
    if (!(await syncReady())) {
        return;
    }

    setStartupNote("正在与云端同步…");
    const unlisten = await listen<SyncProgress>("sync-progress", (event) => {
        const step = event.payload;
        const counter = step.total > 1 ? `（${step.done + 1}/${step.total}）` : "";
        setStartupNote(`正在与云端同步：${step.text}${counter}`);
    });

    try {
        const report = await invoke<SyncReport>("sync_now");
        // 一路顺风就不打扰；有合并过的东西才说一句（那是**动过你的东西**）
        if (report.conflicts.length > 0) {
            flash(
                `同步完成：有 ${report.conflicts.length} 个文件两边都改过，已按时间取了新的那版`,
            );
        }
    } catch (error) {
        flash(`同步没成功：${error}（先用本机的数据，之后可以在设置里再同步一次）`);
    } finally {
        unlisten();
        setStartupNote("正在打开工作目录…");
    }
}

/**
 * 关窗之前把这一趟送出去。
 *
 * **不打断关闭**：失败了也照样关（东西在本机，下次同步还在），只是说明白。
 */
export async function syncBeforeClose(): Promise<void> {
    if (!(await syncReady())) {
        return;
    }
    closing.value = true;
    try {
        const report = await run();
        if (report && report.uploaded + report.downloaded > 0) {
            // 来不及让人看浮条了（窗口马上就没），所以只写日志
            console.info("关窗前同步完成：", report);
        }
    } finally {
        closing.value = false;
    }
}
