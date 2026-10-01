/**
 * 偏好的**运行时那一份**。
 *
 * 启动时从工作目录读一次（`open_workspace`），之后改了就地生效、并节流落盘。
 * 组件不直接问后端，都走这里 —— 「谁在用这份偏好」只有一处。
 */

import { computed, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { applyAppearance } from "./appearance.ts";
import type { MaintenanceInfo } from "../ipc/maintenance.ts";
import type { Policy } from "../ipc/note.ts";
import type {
    DatabaseMeta,
    Preferences,
    Star,
    ThemeMode,
    WorkspaceInfo,
} from "../ipc/settings.ts";
import { markStartupReady, reportStartupFailure, startupPhase } from "./startup.ts";

/** 默认值要与 Rust 端 `Preferences::default()` 一致 */
export const preferences = ref<Preferences>({
    zoom: 1,
    accent: "#5b8dd6",
    theme: "system",
    limit_width: true,
    rail_collapsed: false,
    code_line_numbers: true,
    record_history: true,
    starred: [],
});

/** 工作目录与数据库的位置（诊断页显示"东西存在哪"） */
export const workspaceRoot = ref("");
export const databaseRoot = ref("");
export const databaseMeta = ref<DatabaseMeta | null>(null);

/**
 * 整理设置（回收站留多少天、自动整理隔多少天、上次各是什么时候）。
 *
 * 它在**仓库**里（`settings/repository.json`），不在偏好里 —— 换台机器读同一份仓库，
 * 这两个期限也该跟着走。所以它与偏好分开存，重新读一次才更新。
 */
export const maintenance = ref<MaintenanceInfo>({
    trash_keep_days: 30,
    gc_interval_days: 1,
    last_trash_purge: "",
    last_gc: "",
});

/**
 * 重新读一遍仓库那一层的信息（工作目录、数据库、整理设置）。
 *
 * 启动时读一次；跑完整理之后还要再读一次 —— 上次执行时间变了，页面上那两行得跟上。
 */
export async function refreshWorkspaceInfo(): Promise<void> {
    const info = await invoke<WorkspaceInfo>("open_workspace");
    workspaceRoot.value = info.root;
    databaseRoot.value = info.database_root;
    databaseMeta.value = info.meta;
    protection.value = info.protection;
    gpgAvailable.value = info.gpg_available;
    maintenance.value = info.maintenance;
}

/** 这台计算机上有没有 gpg（没有时签名 / 加密的选项不可用） */
export const gpgAvailable = ref(false);

/**
 * 新内容落盘时的**仓库默认**保护策略。
 *
 * 它**不是偏好**，是仓库自己的设置（`settings/repository.json`）：它决定数据长什么样，
 * 跟着仓库走，而不是跟着这台机器走。已经写过的笔记照它自己最新一版粘住，
 * 它只对还没有正文的新笔记生效。
 */
export const protection = ref<Policy>({
    compress: true,
    gpg_sign: null,
    gpg_encrypt: null,
    symmetric: false,
});

/**
 * 标签栏**此刻**展不展开。
 *
 * 与 `preferences.rail_collapsed` 是两件事，别混：
 *
 * - `preferences.rail_collapsed` 是**下次打开的默认值**，在设置页里改，会落盘；
 * - 这个是**这一次的样子**，在标签栏上那个按钮改，只活在内存里。
 *
 * 展开/收起是高频操作，每点一下写一次文件不值当。
 */
export const railCollapsed = ref(false);

/** 标签栏上那个按钮：只改这一次的样子，不落盘 */
export function toggleRail(): void {
    railCollapsed.value = !railCollapsed.value;
}

/**
 * 落盘节流：缩放可能是拖着滑杆、或按住 Ctrl 滚轮连发的，一次改一点就写一次文件
 * 既慢也没意义。
 */
const SAVE_DELAY_MS = 300;

let saveTimer: number | undefined;
let dirty = false;

/**
 * 启动时打开工作目录并读回偏好。
 *
 * 打不开（目录建不出来、数据库认不出来）就报给启动失败那条线：主区域换成错误页，
 * 界面不再往下走 —— 这种状态继续用下去，可能把东西写进一个不该写的目录。
 */
export async function openWorkspace(): Promise<void> {
    try {
        const info = await invoke<WorkspaceInfo>("open_workspace");
        workspaceRoot.value = info.root;
        databaseRoot.value = info.database_root;
        databaseMeta.value = info.meta;
        preferences.value = info.preferences;
        protection.value = info.protection;
        gpgAvailable.value = info.gpg_available;
        maintenance.value = info.maintenance;
        // 开局的样子照默认值来；之后在标签栏上怎么开合都不再动它
        railCollapsed.value = info.preferences.rail_collapsed;
        markStartupReady();
    } catch (error) {
        // 这一步包含了建目录与认数据库，所以名字把两件事都说上
        reportStartupFailure("打开工作目录与数据库", error);
    }

    // 失败时也应用一次：错误页本身也要有对的配色
    applyAppearance(preferences.value);
}

export function updatePreferences(patch: Partial<Preferences>): void {
    preferences.value = { ...preferences.value, ...patch };
    applyAppearance(preferences.value);
    dirty = true;
    // 缩放可能是拖着滑杆、或按着 Ctrl 滚轮连发的，等手停；其余点一下就定，立刻写，
    // 免得"改完马上关窗口"落在节流窗口里丢掉。
    if ("zoom" in patch) {
        scheduleSave();
    } else {
        void persist();
    }
}

/** 星标过的页面（新标签页显示它） */
export const starred = computed(() => preferences.value.starred);

export function isStarred(address: string): boolean {
    return preferences.value.starred.some((star) => star.address === address);
}

/**
 * 加/去星标。地址是**规范地址** —— 星标说的是"这一页"，不是"这一串字"，
 * 所以标题改了、命名空间改名了，星标都还认得。
 */
export function toggleStar(address: string, title: string): void {
    if (!address) {
        return;
    }
    const kept = preferences.value.starred.filter((star) => star.address !== address);
    const next: Star[] =
        kept.length === preferences.value.starred.length
            ? [...kept, { address, title }]
            : kept;
    updatePreferences({ starred: next });
}

/** 循环切换的顺序：跟随系统 → 浅色 → 深色 → 跟随系统 */
const THEME_CYCLE: ThemeMode[] = ["system", "light", "dark"];

/** 换到下一个深浅色（右下角那组按钮里的一个） */
/**
 * 忘掉这一篇在这次会话里存过的口令（它的每一版）。
 *
 * 口令只在内存里，所以"忘掉"就是丢掉 —— 下次读它要重新输入。
 */
export async function forgetPassphrase(title: string): Promise<void> {
    await invoke("forget_passphrase", { title });
}

export function cycleTheme(): ThemeMode {
    const at = THEME_CYCLE.indexOf(preferences.value.theme);
    // 找不到当前模式时 at 是 -1，(0 % 3) 落在第一项，天然是个安全兜底
    const next = THEME_CYCLE[(at + 1) % THEME_CYCLE.length] ?? "system";
    updatePreferences({ theme: next });
    return next;
}

/**
 * 改**仓库默认**的保护策略：往后**新建的笔记**从它出发；
 * 已经写过的笔记照它自己最新一版粘住，不受影响。
 */
export async function setProtection(next: Policy): Promise<void> {
    await invoke("set_protection", { protection: next });
    protection.value = next;
}

/**
 * 给某一版解锁（`reference` 是地址里的版本 token，`null` = 最新版）。
 *
 * 口令按版本存：每篇可以用不同的密码，所以在别的页解过锁不等于这一页能读。
 * 它**从不落盘** —— 只活在内存里，程序一关就没了。
 */
export async function unlock(title: string, reference: string | null, passphrase: string): Promise<void> {
    await invoke("unlock", { title, reference, passphrase });
}

/** 忘掉这次会话里的全部口令 */
export async function lockAll(): Promise<void> {
    await invoke("lock");
}

/** 立刻落盘（关窗前用），不等节流 */
export async function flushPreferences(): Promise<void> {
    window.clearTimeout(saveTimer);
    saveTimer = undefined;
    await persist();
}

function scheduleSave(): void {
    window.clearTimeout(saveTimer);
    saveTimer = window.setTimeout(() => void persist(), SAVE_DELAY_MS);
}

/**
 * 整份写回。
 *
 * 没改动过就不写 —— 关窗前那次 `flushPreferences` 不该白白写一遍文件。
 * 落盘失败不打断使用：这一次改动没存住，比弹一个错误挡住人强；脏位留着，
 * 下一次改动会连它一起重试。
 */
async function persist(): Promise<void> {
    if (!dirty) {
        return;
    }

    // 启动还没跑完就一个字都不写：那时连"这个目录是不是我们的"都还没确认
    if (startupPhase.value !== "ready") {
        return;
    }

    try {
        await invoke<Preferences>("save_preferences", {
            preferences: preferences.value,
        });
        dirty = false;
    } catch (error) {
        console.warn("存偏好失败：", error);
    }
}
