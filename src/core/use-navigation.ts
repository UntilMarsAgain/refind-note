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
 * 窗口里的**导航胶水层**：界面各处说"我想去哪儿"，由这里把它翻译成标签页动作。
 *
 * 这些函数本身几乎不含状态，它们是**菜单、地址栏、正文链接、编辑器、历史回退
 * 各自的口语 与 `useTabs()` 里的正规叫法之间的翻译层**。所以：
 *
 * - 标签页状态**只有一份**（`useTabs()`，由 `App.vue` 调一次），
 *   本模块**不自己再调一次 `useTabs()`**，而是把需要的那几项当参数收进来。
 *   一旦在这里再调一次，窗口里就会有两套标签页，三块读的不是同一份 —— 那是本项目
 *   明说的架构前提（「三块之间没有谁驱动谁，它们只是读同一份标签页状态」），不能破。
 * - 这一层为什么与 `App.vue` 分开：它是最"叙事"的一块（什么时候该压历史、
 *   什么时候该替换、解锁页退出去该去哪），全部集中在一起才读得下来；
 *   而 `App.vue` 剩下的部分是**装配**（谁挂在模板哪个位置、开机那一轮、监听摘除），
 *   两者变更的原因完全不同。
 * - 它放在 `core/` 而不是新建 `composables/`：它只依赖 `core/tabs.ts` 的类型
 *   与 `core/address.ts` / `core/notice.ts`，与 `core/` 其余模块是同一层关系；
 *   `dom/` 是碰 DOM 的、`ipc/` 是通后端的，都不是它的归属。
 */

import { invoke } from "@tauri-apps/api/core";
import type { ComputedRef } from "vue";
import { withSection } from "./address.ts";
import { flash } from "./notice.ts";
import type { Movement, TabState } from "./tabs.ts";

/**
 * 本层需要的那几项标签页能力。
 *
 * 刻意只列用得着的（而不是整个 `useTabs()` 的返回类型）：多写一个字段进来，
 * 就多一条"这一层其实还依赖了别的东西"的暗示 —— 而这一层不该依赖别的东西。
 */
export interface NavigationTabs {
  /** 当前标签页 */
  active: ComputedRef<TabState | null>;
  /** 当前标签页的规范地址（拼章节时以它为前缀） */
  committed: ComputedRef<string>;
  /** 退得回去吗（走不通时的兜底要看它） */
  canGoBack: ComputedRef<boolean>;
  /** 导航：解析交给后端，前端不认语法 */
  navigate: (input: string, movement?: Movement) => Promise<boolean>;
  /** 在新标签页里打开某处 */
  newTabWith: (address: string) => Promise<void>;
  /** 后退一步 */
  goBack: () => Promise<void>;
}

/** 窗口里的导航胶水层。参数就是它要用的那几项标签页能力 */
export function useNavigation(tabs: NavigationTabs) {
  const { active, committed, canGoBack, navigate, newTabWith, goBack } = tabs;

  /** 标题栏那颗首页按钮：回新标签页（它也是一个地址，走同一条导航） */
  function openHome() {
    void navigate("special:newtab", "push");
  }

  /** 标签栏底下的入口：它也是地址（`special:xxx`），算一次跳转 */
  function openSpecial(page: string) {
    void navigate(`special:${page}`, "push");
  }

  /** 正文里点了内部链接：算一次跳转 */
  function openNote(title: string) {
    void navigate(title, "push");
  }

  /**
   * 页面上锁时改去 `@unlock`。
   *
   * 走**替换**而不是压新记录：从解锁页后退回来会又落到这一页、又被送去解锁，
   * 来回打转。换掉脚下这条之后，后退回到的还是进来之前那一页。
   */
  function redirectNote(input: string) {
    void navigate(input, "replace");
  }

  /** 正文里的内部链接被 Ctrl/Cmd 点击：在新标签页打开 */
  function openNoteInNewTab(title: string) {
    void newTabWith(title);
  }

  /**
   * 正文里点了页内锚点：把章节叠进当前地址。
   *
   * 前缀（名称、状态）原样保留，只换 `#` 之后的部分 —— 章节算一次跳转，能后退回来。
   */
  function openSection(section: string) {
    void navigate(withSection(committed.value, section), "push");
  }

  /** 编辑器提交完成或取消：回到它给的阅读地址 */
  function leaveEditor(input: string) {
    void navigate(input, "push");
  }

  /**
   * 去看历史里的某一版。
   *
   * 这里拼的是**输入**（`标题@view-3`），和人在地址栏里敲的是同一种东西 ——
   * 规范地址仍然由后端解析出来。
   */
  function openVersion(payload: { title: string; rev: number }) {
    void navigate(`${payload.title}@view-${payload.rev}`, "push");
  }

  /** 从历史清单直接去回退页 —— 这条路不用先读得懂那一版 */
  function rollbackVersion(payload: { title: string; rev: number }) {
    void navigate(`${payload.title}@rollback-${payload.rev}`, "push");
  }

  /** 删除完成：这篇已经没了，改去全部页面 */
  function afterDelete() {
    void navigate("special:all", "push");
  }

  /** 当前页对应的笔记标题（确认页、解锁页自己也属于某一篇） */
  function currentNoteTitle(): string {
    const outcome = active.value?.route?.outcome;
    return outcome?.kind === "note" ? outcome.title : "";
  }

  /**
   * 建一篇笔记，建完直接进编辑器。
   *
   * 空白笔记没有什么可读的，所以下一步就是 `@edit`；用的名字是后端回来的**规范标题**，
   * 不是人敲进去的那一串。
   */
  async function createNote(title: string) {
    try {
      const created = await invoke<string>("create_note", { title });
      await navigate(`${created}@edit`, "push");
    } catch (reason) {
      // 建不出来（重名、名字不合法）要说给人听，而不是默默什么都不发生
      flash(String(reason));
    }
  }

  /**
   * 某一页解开了锁：把脚下这条 `@unlock` **换成那一页本身**。
   *
   * 为什么不"退一步"：进来时那条记录已经被替换成 `@unlock` 了（见 `redirectNote`），
   * 后退一步到的是**再往前**那一页 —— 从新标签页点进来的人会发现自己又回到了新标签页，
   * 而刚解开的那一篇就在眼前却看不成。所以这里原地替换成那一页：
   * 后退依旧回得来处，而眼前正是要读的东西。
   *
   * 一个例外：从**编辑器**点「去解锁」过来的（那条是压进去的，不是替换的），
   * 解完退回编辑器才对 —— 那里本来就是要接着写的地方。
   */
  function afterUnlock() {
    const tab = active.value;
    const route = tab?.route;
    const outcome = route?.outcome;

    if (!tab || !route || outcome?.kind !== "note") {
      // 认不出来是哪一篇（理论上到不了）：退回上一条就是最合理的
      if (canGoBack.value) {
        void goBack();
      }
      return;
    }

    const title = outcome.title;
    const before = tab.cursor > 0 ? tab.history[tab.cursor - 1] : undefined;
    if (before === `${title}@edit`) {
      void goBack();
      return;
    }

    // 看的是旧版本就回到那一版（`@unlock-3` 解的是第 3 版）
    const mode = route.address.mode;
    const reference = mode.kind === "unlock" ? mode.ref : null;
    void navigate(reference ? `${title}@view-${reference}` : title, "replace");
  }

  /**
   * 读不出来的页面留的那条出路：退一步。
   *
   * 后退回得去就后退（绝大多数时候是）；整个标签页就是从这一页开局的，那就去全部页面。
   */
  function leavePage() {
    if (canGoBack.value) {
      void goBack();
      return;
    }
    void navigate("special:all", "push");
  }

  /**
   * 从解锁页退出来。
   *
   * 进这一页时那条记录已经被**替换**掉了（原来那一页读不出来，留着它只会再被送回来），
   * 所以"退一步"就是后退：回到进来之前待的地方。一整个标签页都是从 `@unlock` 开局的，
   * 没处可退，就摆到全部页面上让人挑。
   */
  function leaveUnlock() {
    if (canGoBack.value) {
      void goBack();
      return;
    }
    void navigate("special:all", "push");
  }

  /**
   * 回滚完成：回到阅读地址，让人直接看到回滚后的内容。
   *
   * 特意把"旧历史一条未动"说清楚 —— 回滚看起来像"把东西改回去了"，
   * 不说的话人会以为后面那几版没了。
   */
  function afterRollback(rev: number) {
    flash(`已回滚至所选版本（第 ${rev} 版），原有版本记录均保留`);
    const title = currentNoteTitle();
    if (title) {
      void navigate(title, "push");
    }
  }

  return {
    openHome,
    openSpecial,
    openNote,
    redirectNote,
    openNoteInNewTab,
    openSection,
    leaveEditor,
    openVersion,
    rollbackVersion,
    afterDelete,
    currentNoteTitle,
    createNote,
    afterUnlock,
    leavePage,
    leaveUnlock,
    afterRollback,
  };
}
