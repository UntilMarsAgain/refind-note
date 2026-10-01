/**
 * 同步的**时机**都收在这一处。
 *
 * 什么在什么时候同步，是这个模块唯一要回答的问题：
 *
 * - **启动时**：`syncAtStartup`，排在打开数据库之前（理由见 `preferences.ts`）；
 * - **提交之后**：`requestSyncAfterCommit`，攒一会儿再跑 —— 连着提交几次只同步一次；
 * - **手动**：`syncNow`，设置页那两颗按钮（「立即同步」，以及不等云端那把锁的「强制同步」）；
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

/** 同步开着吗、配置填全了吗（后端说了算；界面据此决定摆不摆那颗按钮） */
const available = ref(false);

export const syncAvailable = readonly(available);

/** 正在同步（同一时刻只跑一次） */
const busy = ref(false);
/** 跑到哪儿了（进度条/加载页用） */
const progress = ref<SyncProgress | null>(null);
/** 关窗之前那一次：界面据此摆一块遮罩，别让人对着没反应的窗口再点一次 */
const closing = ref(false);

export const syncBusy = readonly(busy);
export const syncProgress = readonly(progress);
export const syncClosing = readonly(closing);

/**
 * 提交之后隔多久**开始**同步：让"连着改几下"落定。
 *
 * 这一小段是**防抖**（每来一次就重新计时）—— 它只影响"什么时候开始"，
 * 不影响"最后那次改动传不传得上去"。
 */
const SETTLE_MS = 3_000;

/**
 * 两次同步之间至少隔多久。
 *
 * 同步本身要跑一会儿（扫本地、列云端、挨个传），刚跑完又跑没有意义。
 * 关键是冷却期间来的改动**不丢**：记一笔，冷却一结束立刻补跑一趟
 * （见 `pendingAfterCooldown`）—— 这就是"最后一个一定传得上去"的保证。
 */
const COOLDOWN_MS = 30_000;

/** 落定计时器 */
let settleTimer: number | undefined;
/** 冷却到什么时候为止 */
let cooldownUntil = 0;
/** 冷却期间又有人叫过：到点补跑一趟 */
let pendingAfterCooldown = false;
/** 正在跑的时候又有人叫：跑完再跑一趟（不是丢掉） */
let queued = false;

/**
 * 问一遍"能不能同步"，并记住 —— 标题栏那颗按钮与启动那一步都看它。
 *
 * 设置页改完设置要叫一次（刚填好桶名，那颗按钮就该出现）。
 */
export async function refreshSyncAvailability(): Promise<boolean> {
    try {
        available.value = await invoke<boolean>("sync_ready");
    } catch (error) {
        console.warn("问同步状态失败：", error);
        available.value = false;
    }
    return available.value;
}

/** 上一问的答案（不重新问） */
export function syncReady(): boolean {
    return available.value;
}

/**
 * 跑一次。
 *
 * 同一时刻只有一趟：正在跑的时候再叫，就在跑完之后**再补一趟**（不是丢掉——
 * 那期间可能正好提交了新东西）。
 *
 * `force` = 不等云端那把锁（见 [`syncNow`]）。
 *
 * 跑不成会**抛出来**：谁叫的谁负责说给人听 —— 自动那几条路（启动、提交后、关窗前）
 * 自己兜住，设置页写在自己那一栏里，标题栏那颗按钮弹浮条。
 */
async function run(force: boolean): Promise<SyncReport | null> {
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
        return await invoke<SyncReport>("sync_now", { force });
    } finally {
        unlisten();
        busy.value = false;
        progress.value = null;
        if (queued) {
            queued = false;
            // 补的那一趟是自动那一路来的（提交后攒下的）：没人守着看，失败就说一句
            void run(false).catch((error) => flash(`同步没成功：${error}`));
        }
    }
}

/**
 * 现在同步一次（设置页那颗按钮、标题栏那颗云）。
 *
 * `force` 是"不等了"：云端那把锁还热着也**直接抢过来**。另一台机器崩在半路时，
 * 普通同步会让你干等它超时 —— 这条路是给那种时候用的，代价写在设置页上
 * （另一边要真在同步，两边就撞上了）。
 *
 * 刚下载回来的东西，**当前打开着的页面看不见** —— 那几页还拿着旧内容。
 * 所以取回来之后说一句，让人知道该把页面重新打开。
 */
export async function syncNow(options: { force?: boolean } = {}): Promise<SyncReport | null> {
    return run(options.force === true);
}

/**
 * 提交之后叫一次。
 *
 * 节奏是这么定的：
 *
 * 1. **先等 [`SETTLE_MS`] 落定**：连着一串改动（提交、传图、再提交）只触发一次；
 *    每来一次就重新计时 —— 这一步是防抖，但它只决定"什么时候开始"。
 * 2. **跑完进入 [`COOLDOWN_MS`] 冷却**：免得刚跑完又跑。
 * 3. 冷却期间来的改动**记一笔**（`pendingAfterCooldown`），到点立刻补跑 ——
 *    所以"后面那次改动传不上去"不会发生；代价是最多等一个冷却周期。
 *
 * 另外三条路兜底：启动时、手动、关窗前都会同步一次 —— 定时器还没到点也不怕。
 * 没开同步时这里**什么都不做**（提交本身与同步无关）。
 */
export function requestSyncAfterCommit(): void {
    window.clearTimeout(settleTimer);
    settleTimer = window.setTimeout(() => void runWhenFree(), SETTLE_MS);
}

/** 冷却过了就跑一趟；没过就记一笔，到点再来 */
async function runWhenFree(): Promise<void> {
    const wait = cooldownUntil - Date.now();
    if (wait > 0) {
        pendingAfterCooldown = true;
        window.setTimeout(() => {
            if (pendingAfterCooldown) {
                pendingAfterCooldown = false;
                void runWhenFree();
            }
        }, wait + 50);
        return;
    }

    if (!(await refreshSyncAvailability())) {
        return;
    }
    // 这条路没人守着看（是提交之后自己叫的），失败就说一句
    try {
        await run(false);
    } catch (error) {
        flash(`同步没成功：${error}`);
    }
    // 冷却期间攒下的改动（如果有）上面那个定时器会接手
    cooldownUntil = Date.now() + COOLDOWN_MS;
}

/**
 * 启动时同步一次（开着的话）——**要在打开数据库之前**跑，理由见 `preferences.ts`。
 *
 * 失败不挡启动：网断了、桶名写错了、锁被别人拿着，都不该让人打不开自己的笔记。
 */
export async function syncAtStartup(): Promise<void> {
    if (!(await refreshSyncAvailability())) {
        return;
    }

    setStartupNote("正在与云端同步…");
    const unlisten = await listen<SyncProgress>("sync-progress", (event) => {
        const step = event.payload;
        const counter = step.total > 1 ? `（${step.done + 1}/${step.total}）` : "";
        setStartupNote(`正在与云端同步：${step.text}${counter}`);
    });

    try {
        const report = await run(false);
        // 一路顺风就不打扰；有合并过的东西才说一句（那是**动过你的东西**）
        if (report && report.conflicts.length > 0) {
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
    if (!(await refreshSyncAvailability())) {
        return;
    }
    closing.value = true;
    try {
        const report = await run(false);
        if (report && report.uploaded + report.downloaded > 0) {
            // 来不及让人看浮条了（窗口马上就没），所以只写日志
            console.info("关窗前同步完成：", report);
        }
    } catch (error) {
        // 窗口这就没了，浮条没人看得见；东西在本机，下次同步接着来
        console.warn("关窗前同步没成功：", error);
    } finally {
        closing.value = false;
    }
}
