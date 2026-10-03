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
 * 全局快捷键：把一次按键翻译成一个**动作 id**。
 *
 * ## 为什么这里不再写死分支
 *
 * 原来这里是三条写死的 `if`（Ctrl+T / Shift+T / Ctrl+W）。加了可改键之后那样不行：
 * 用户把"新建标签页"改成 `Alt+N`，这里还认 `Ctrl+T` 就等于这个动作废了。
 *
 * 所以这一层只做一件事 —— **按键 → 动作 id**，认键的规则在 `core/keymap.ts` 一处。
 * 拿到 id 之后干什么，由 [`useShortcuts`] 的调用方决定（`App.vue` 拿它分派）。
 *
 * ## 编辑器里的按键不算
 *
 * CodeMirror 的输入框、选区那些按键归它自己（它自己的键位表比我们全），我们抢过来
 * 只会把它弄坏。所以来自编辑器内部的事件直接放过 —— 判断看事件目标在不在编辑器里，
 * 那是 CM6 的惯例（`.cm-editor` / `.cm-content`）。
 *
 * 但**我们自己的**动作（页内查找那些）即使是编辑器里的按键也照样认 —— 用户在编辑器里
 * 按 Ctrl+F 找东西是合理的，见 [`shouldIgnore`] 的注释。
 */

import type { Ref } from "vue";
import { ACTIONS, bindingOf, matches } from "../core/keymap.ts";

/** 这一层要用的标签页能力：与 `use-navigation` 同样只列用得着的 */
export interface ShortcutTabs {
  /** 当前是第几个标签页（关掉的就是它） */
  activeIndex: Ref<number>;
  /** 开一个新标签页 */
  newTab: () => Promise<void>;
  /** 关掉某个标签页 */
  close: (index: number) => void;
  /** 退回来上一个刚关掉的标签页 */
  reopenClosed: () => void;
}

/**
 * 这些动作**在编辑器里也认**。
 *
 * 它们不是"编辑器自己的事"（选中一段、缩进、括号配对那些是），而是整个窗口的事：
 * 在编辑器里找东西、在编辑器里翻到前一页，都是说得通的。
 */
const ALSO_INSIDE_EDITOR = new Set(["find", "find-next", "find-previous"]);

/**
 * 这次按键要不要放过。
 *
 * 编辑器里的按键，默认放过（那些是 CM6 自己的键）；但 [`ALSO_INSIDE_EDITOR`] 里的
 * 动作不放过 —— 上面说了为什么。
 */
function shouldIgnore(event: KeyboardEvent, actionId: string): boolean {
    if (!isInsideEditor(event.target)) {
        return false;
    }
    return !ALSO_INSIDE_EDITOR.has(actionId);
}

/** 事件目标是不是编辑器内部（CM6 的两个类名是它的惯例） */
function isInsideEditor(target: EventTarget | null): boolean {
    const element = target as HTMLElement | null;
    if (!element || typeof element.closest !== "function") {
        return false;
    }
    // 只认编辑器**内部**：`.cm-content` 是输入的那一层，`.cm-editor` 是它的外壳
    // （面板、菜单之类的在编辑器里，但不在 content 里）
    return element.closest(".cm-editor") !== null;
}

/**
 * 这次按键对应哪个动作（认不出来返回空串）。
 *
 * 遍历动作表而不是写死几个 `if`：用户改键之后这个对照表才是对的。
 * 认不出来也不急 —— 多半是我们没配的动作（复制、粘贴、打印），交给浏览器的默认行为。
 */
function actionFor(event: KeyboardEvent): string {
    for (const action of ACTIONS) {
        if (matches(event, bindingOf(action))) {
            return action.id;
        }
    }
    return "";
}

/**
 * 造出全局按键的处理函数。
 *
 * 标签页那三个动作在这一层直接跑（它们就是标签页的事，参数已经收进来了）；
 * 其余动作交给 `emit` —— 查找、翻页、菜单那些与标签页无关，语义差得远，
 * 不该让这一层知道它们。
 */
export function useShortcuts(tabs: ShortcutTabs, emit: (actionId: string) => void) {
  const { activeIndex, newTab, close, reopenClosed } = tabs;

  /**
   * 跑这个动作，**同步**地答一句"我管不管这个键"。
   *
   * 答"管"才 `preventDefault`。异步的那些（开新标签页要 `await resolveAddress`）
   * 是 `void` 出去的 —— `preventDefault` 必须在事件还没派发完时调用，
   * 放进 `.then()` 就已经晚了，拦不住任何东西。
   */
  function run(actionId: string): boolean {
    switch (actionId) {
      case "new-tab":
        void newTab();
        return true;
      case "reopen-closed":
        reopenClosed();
        return true;
      case "close-tab":
        close(activeIndex.value);
        return true;
      default:
        // 其余动作（查找、翻页、菜单…）不在这一层处理：由 `onAction` 那头接。
        // **不 preventDefault** —— 这里不知道它会不会被处理，先别把默认行为吃掉。
        return false;
    }
  }

  /** 全局按键处理：认出动作就派发出去（`emit` 那边才是真正做事的地方） */
  function onKeydown(event: KeyboardEvent) {
    const actionId = actionFor(event);
    if (!actionId || shouldIgnore(event, actionId)) {
      return;
    }
    if (run(actionId)) {
      event.preventDefault();
      return;
    }
    emit(actionId);
  }

  return { onKeydown, emit };
}