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
 * 键位：**运行时那一份** + 识别与显示的那点逻辑。
 *
 * 动作表是"这个程序有哪些能改键的动作"的唯一出处：设置页照它列行、
 * `useShortcuts` 照它分派、菜单那排小方键照它显示。新增一个动作只改 [`ACTIONS`]。
 *
 * 与偏好一样：启动时读一次，之后改了就地生效并落盘。组件不直接问后端，都走这里。
 */

import { ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import type { Binding, Keymap } from "../ipc/keymap.ts";

/** 一个动作：改键时显示的那一行 */
export interface ShortcutAction {
    /** 存盘用的 id（`keymap.json` 里的键，Rust 那边也认它） */
    id: string;
    /** 界面上显示的名字 */
    label: string;
    /** 出厂键位；必须与 `keymap.rs` 里的 `DEFAULTS` 一致 */
    keys: Binding;
    /** 设置页里的分组 */
    group: string;
}

/**
 * 出厂键位。
 *
 * **必须与 `keymap.rs` 的 `DEFAULTS` 一致** —— 那边是存盘与收进合法范围的地方，
 * 这里是显示与识别的地方。两边不一致的症状很难看（菜单上写着 `Ctrl+F`，
 * 按了没反应），所以 Rust 那边有测试挡着出厂键位互不重叠。
 */
export const ACTIONS: ShortcutAction[] = [
    // —— 标签页
    { id: "new-tab", label: "新建标签页", keys: ["Ctrl", "T"], group: "标签页" },
    { id: "reopen-closed", label: "恢复刚关的标签页", keys: ["Ctrl", "Shift", "T"], group: "标签页" },
    { id: "close-tab", label: "关闭标签页", keys: ["Ctrl", "W"], group: "标签页" },

    // —— 查找
    { id: "find", label: "页内查找", keys: ["Ctrl", "F"], group: "查找" },
    { id: "find-next", label: "下一个", keys: ["F3"], group: "查找" },
    { id: "find-previous", label: "上一个", keys: ["Shift", "F3"], group: "查找" },

    // —— 页面
    // 方向键写 `ArrowLeft` / `ArrowRight`：**`event.key` 就是这个**。
    // 写成 `"Left"` 是老代码里那个已废弃的别名 —— 后果是键位永远匹配不上
    // （提示里写着 Alt+←，按了没反应），而显示那侧因为不认识它，
    // 还会把 `←` 原样打出来，看着倒像是对的。
    { id: "back", label: "后退", keys: ["Alt", "ArrowLeft"], group: "页面" },
    { id: "forward", label: "前进", keys: ["Alt", "ArrowRight"], group: "页面" },
    { id: "home", label: "首页", keys: ["Alt", "H"], group: "页面" },
    { id: "menu", label: "菜单", keys: ["Alt", "M"], group: "页面" },
    // 标签跟着**行为**走，不跟着旧名字走：以前这个动作是整个窗口重载，所以叫"重载"；
    // 现在它与标题栏那颗按钮是同一件事（重读当前这一页），按钮上写"刷新"，
    // 这里就该写"刷新" —— 两处不同名，人会以为是两件事。设置页显示的是这一处的名字。
    { id: "reload", label: "刷新", keys: ["Ctrl", "R"], group: "页面" },
];

/**
 * 用户改过的那些（**稀疏** —— 缺的动作用出厂的）。
 *
 * 启动时后端送来的就是出厂那份（全的）；用户改过之后只送差异，
 * 这样文件平时几乎是空的（见 `keymap.rs` 存盘那一段）。
 */
export const keymap = ref<Keymap>({});

/** 实际生效的那一份（用户改了就用用户的） */
const effective = ref<Keymap>({});

/** 设置页列出的分组顺序（按 `ACTIONS` 里第一次出现的次序，不另维护一份） */
export const SHORTCUT_GROUPS: string[] = [...new Set(ACTIONS.map((action) => action.group))];

/** 某一组里的动作 */
export function actionsInGroup(group: string): ShortcutAction[] {
    return ACTIONS.filter((action) => action.group === group);
}

/** 某个动作**实际生效**的键位 */
export function bindingOf(action: ShortcutAction): Binding {
    return effective.value[action.id] ?? action.keys;
}

/**
 * 某个动作用户**有没有改过**（界面据此决定"恢复默认"是不是可点的）。
 *
 * 判的是**当前键位与出厂的那一串不一样**，不是"表里有没有这一项"。
 *
 * 这个区别很要紧：后端送来的键位表是**出厂那份全的**（`keymap.rs` 里的
 * `Keymap::defaults()`），表里每一项都在。所以按"有没有这一项"来判的话，
 * 刚装好、一个键都没改过的用户会看到**每一行都挂着"恢复默认"** ——
 * 而"恢复默认"该有的那个前提是"你改过它"。
 */
export function isCustomized(action: ShortcutAction): boolean {
    return bindingOf(action).join("+") !== action.keys.join("+");
}

/** 按 id 找动作；不认识返回 null（而不是给个假的 —— 假的会显示成"这个动作存在"） */
export function actionById(id: string): ShortcutAction | null {
    return ACTIONS.find((action) => action.id === id) ?? null;
}

/**
 * 把"某动作现在按什么"记下来并落盘。
 *
 * 存进 `keymap` 的是**稀疏**的那份：改成出厂的就把它从表里删掉，
 * 而不是把出厂的键抄一遍 —— 否则"改回默认"会在文件里留下一条永远等于默认的行，
 * 下次出厂键位一变，那行就成了过时的覆盖。
 */
export async function setBinding(action: ShortcutAction, keys: Binding): Promise<void> {
    const next: Keymap = { ...keymap.value };
    if (keys.join("+") === action.keys.join("+")) {
        delete next[action.id];
    } else {
        next[action.id] = [...keys];
    }
    await persist(next);
}

/** 把某个动作恢复出厂 */
export async function resetBinding(action: ShortcutAction): Promise<void> {
    await setBinding(action, action.keys);
}

/** 落盘并刷新运行时那一份（后端可能又收了一遍，写回来的是它真正的样子） */
async function persist(next: Keymap): Promise<void> {
    const sparse = sparseOf(next);
    keymap.value = sparse;
    effective.value = effectiveOf(await invoke<Keymap>("save_keymap", { keymap: sparse }));
}

/**
 * 把稀疏表与出厂表合成一份完整的。
 *
 * 出厂那份是权威的默认值：用户文件里**没有**的动作（以及不认识的动作）由它兜着。
 */
function effectiveOf(map: Keymap): Keymap {
    const out: Keymap = {};
    for (const action of ACTIONS) {
        const keys = map[action.id];
        out[action.id] = keys && keys.length > 0 ? [...keys] : [...action.keys];
    }
    return out;
}

/**
 * 只留**与出厂不同**的那些项。
 *
 * 后端送来的表是**出厂那份全的**（`keymap.rs` 里 `Keymap::defaults()` 是完整一张），
 * 而存盘要的是**稀疏**的：文件平时几乎是空的，手改时看得懂，将来出厂键位一改，
 * 那份全表里的旧值就会变成过时覆盖。
 *
 * 所以这张表**每次进出都要过这一遍**：进去（存盘前）滤成稀疏，出来（读回后）也滤成
 * 稀疏 —— 否则 `setBinding` 拿 `keymap.value` 一摊开，存下去的就是全表。
 */
function sparseOf(map: Keymap): Keymap {
    const out: Keymap = {};
    for (const action of ACTIONS) {
        const keys = map[action.id];
        // 不认识的动作顺手也丢了：它只可能来自手改或旧版本，留着只会让人困惑
        if (!keys || keys.length === 0) {
            continue;
        }
        if (keys.join("+") !== action.keys.join("+")) {
            out[action.id] = [...keys];
        }
    }
    return out;
}

/** 启动时读一次 */
export async function loadKeymap(): Promise<void> {
    try {
        // 滤成稀疏：后端送的是出厂全表，而 `keymap.value` 要的是"用户改过的那些"
        const stored = sparseOf(await invoke<Keymap>("load_keymap"));
        keymap.value = stored;
        effective.value = effectiveOf(stored);
    } catch (reason) {
        // 读不出来不该让界面少一堆能按的东西 —— 退回出厂
        console.warn("取键位表失败，这一轮用出厂的：", reason);
        keymap.value = {};
        effective.value = effectiveOf({});
    }
}

// ------------------------------------------------------------------ 识别

/**
 * 这个按键是不是某个动作的键位。
 *
 * **修饰键必须完全一致**，不是"至少按了那些"。少按一个 Shift 就该是另一个键 ——
 * `Ctrl+T` 与 `Ctrl+Shift+T` 是两个动作，糊在一起就有一个按不出来。
 * 多按一个 CapsLock 也算不一致：它不影响字母的大小写语义之外的事，
 * 但用户看到 `Ctrl+T` 按不出、`CapsLock+Ctrl+T` 反而能按，会一头雾水。
 *
 * 只读、不改状态。
 */
export function matches(event: KeyboardEvent, binding: Binding): boolean {
    if (binding.length === 0) {
        return false;
    }

    const pressed = pressedKeys(event);
    if (!pressed) {
        return false;
    }
    return binding.join("+") === pressed.join("+");
}

/**
 * 一次按键对应的键序列，与 [`bindingOf`] 存下的**同一种写法**（修饰键在前）。
 *
 * 按不出来（单独按一个 `Shift`、一个 `Ctrl`）返回 `null`：那种键位本来就没意义，
 * 捕捉时应当忽略它，而不是让用户把"只按 Ctrl"绑成某个动作。
 */
export function pressedKeys(event: KeyboardEvent): Binding | null {
    const modifiers: Binding = [];
    if (event.ctrlKey) {
        modifiers.push("Ctrl");
    }
    if (event.shiftKey) {
        modifiers.push("Shift");
    }
    if (event.altKey) {
        modifiers.push("Alt");
    }
    if (event.metaKey) {
        modifiers.push("Meta");
    }
    if (event.getModifierState("CapsLock")) {
        modifiers.push("CapsLock");
    }

    const key = normalizeKey(event.key);
    // 只有修饰键、没有主键：不算一次按键（等下一个键）
    if (!key) {
        return null;
    }
    return [...modifiers, key];
}

/** 这些键已经记进修饰键里了，不能再当"主键"（否则只按 Ctrl 会算出 `Ctrl+Ctrl`） */
const MODIFIER_KEYS = new Set(["Control", "Shift", "Alt", "Meta", "CapsLock", "OS"]);

/** `event.key` 的写法统一到与存盘时一样（`" "` → `"Space"`、大小写归一） */
function normalizeKey(key: string): string | null {
    // 只按下一个修饰键时不算一次按键：`event.key` 是那个修饰键本身，而它已经
    // 被记进上面的修饰键列表了，当主键用会算成 `Ctrl+Ctrl` 之类
    if (MODIFIER_KEYS.has(key)) {
        return null;
    }
    switch (key) {
        case " ":
            return "Space";
        case "Escape":
            return "Esc";
        default:
            break;
    }
    // 单个字母按大小写都该是同一个键位（Shift 已经单独记在修饰键里了）
    if (key.length === 1) {
        return key.toUpperCase();
    }
    if (!key) {
        return null;
    }
    return key;
}

// ------------------------------------------------------------------ 显示

/**
 * 键位显示成 `Ctrl+Shift+T`。
 *
 * macOS 上把 `Meta` 显示成 `Cmd`、`Alt` 显示成 `Option` —— 那是那一族机器上
 * 键盘上**真的印着**的字，写 `Ctrl` 会让人低头找。
 */
export function label(binding: Binding): string {
    return binding.map(displayName).join("+");
}

/** 浮动提示里说的那个名字（菜单右侧那排小方键用它） */
function displayName(key: string): string {
    switch (key) {
        case "Meta":
            return IS_MAC ? "Cmd" : "Win";
        case "Alt":
            // Option 只在 Mac 上写（那台机器的键上印的是 Option/⌥）
            return IS_MAC ? "Option" : "Alt";
        case "ArrowLeft":
            return "←";
        case "ArrowRight":
            return "→";
        case "ArrowUp":
            return "↑";
        case "ArrowDown":
            return "↓";
        default:
            return key;
    }
}

/** macOS 上习惯 Command，不习惯 Ctrl —— 决定显示成 Cmd 还是 Ctrl */
const IS_MAC =
    typeof navigator !== "undefined" && /mac/i.test(navigator.platform || navigator.userAgent);