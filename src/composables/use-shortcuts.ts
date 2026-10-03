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
 * 浏览器习惯的那几条快捷键（Ctrl/Cmd + T / Shift+T / W）。
 *
 * 与 `use-navigation` 同理：这一层要做的只是**把按键翻译成标签页动作**，
 * 标签页状态仍然只有 `useTabs()` 那一份，所以能力从参数收进来，不在这里另调。
 *
 * 之所以单独成文件而不是塞进 `use-navigation`：这两块的**变更原因不同**。
 * 导航那一层跟着"页面之间怎么走"变（页面类型、后端地址格式）；这一层只跟着
 * "还想支持哪些快捷键"变。而且它天然是一个可以直接挂 `window` 的 handler，
 * 与导航的语义毫无关系。
 */

import type { Ref } from "vue";

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

/** 造出那几条快捷键的按键处理函数 */
export function useShortcuts(tabs: ShortcutTabs) {
  const { activeIndex, newTab, close, reopenClosed } = tabs;

  /** 浏览器习惯的快捷键 */
  function onKeydown(event: KeyboardEvent) {
    if (!(event.ctrlKey || event.metaKey)) {
      return;
    }
    const key = event.key.toLowerCase();

    // Ctrl+Shift+T 要先判：否则会被下面的 Ctrl+T 吃掉
    if (key === "t" && event.shiftKey) {
      event.preventDefault();
      reopenClosed();
      return;
    }
    if (key === "t") {
      event.preventDefault();
      void newTab();
      return;
    }
    if (key === "w") {
      event.preventDefault();
      close(activeIndex.value);
    }
  }

  return { onKeydown };
}
