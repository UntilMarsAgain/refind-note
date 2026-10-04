<!--
  Refind Note is a note-taking software.
  Copyright (C) 2026 Until Mars Again

  This program is free software: you can redistribute it and/or modify
  it under the terms of the GNU Affero General Public License as published by
  the Free Software Foundation, either version 3 of the License, or
  (at your option) any later version.

  This program is distributed in the hope that it will be useful,
  but WITHOUT ANY WARRANTY; without even the implied warranty of
  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
  GNU Affero General Public License for more details.

  You should have received a copy of the GNU Affero General Public License
  along with this program.  If not, see <http://www.gnu.org/licenses/>.
-->

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, useTemplateRef, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import AppMenu from "./components/shell/AppMenu.vue";
import ContextMenu from "./components/shell/ContextMenu.vue";
import DebugPanel from "./components/shell/DebugPanel.vue";
import FloatingTools from "./components/shell/FloatingTools.vue";
import RenderPane from "./components/shell/RenderPane.vue";
import StartupError from "./components/shell/StartupError.vue";
import StartupLoading from "./components/shell/StartupLoading.vue";
import TabRail from "./components/shell/TabRail.vue";
import WindowResizeHandles from "./components/shell/WindowResizeHandles.vue";
import WindowTitleBar from "./components/shell/WindowTitleBar.vue";
import { listen } from "@tauri-apps/api/event";
import { addressFromDeepLink } from "./core/deep-link.ts";
import type { HelpPage } from "./ipc/help.ts";
import { loadBrowsing } from "./core/browsing.ts";
import { dismissNotice, flash, notice } from "./core/notice.ts";
import { syncBeforeClose, syncClosing, syncProgress } from "./core/sync.ts";
import { currentWindow } from "./core/window-api.ts";
import { setOpenInNewTab } from "./dom/note-html.ts";
import { syncNowAndReport } from "./core/sync.ts";
import { isMobile } from "./core/platform.ts";
import { openEditorSearch } from "./dom/editor-setup.ts";
import {
  cycleTheme,
  flushPreferences,
  openWorkspace,
  preferences,
  refreshWorkspaceInfo,
  updatePreferences,
} from "./core/preferences.ts";
import { restartStartup, startupPhase } from "./core/startup.ts";
import { loadKeymap } from "./core/keymap.ts";
import { formatBytes, type MaintenanceReport } from "./ipc/maintenance.ts";
import { useTabs } from "./core/tabs.ts";
import { useNavigation } from "./composables/useNavigation.ts";
import { useShortcuts } from "./composables/useShortcuts.ts";
import { installWheelZoom } from "./dom/zoom-wheel.ts";

/**
 * 窗口骨架。
 *
 * - `WindowTitleBar` 绘制标题栏，与「当前标签页的地址」双向绑定；
 * - `TabRail` 绘制垂直标签页；
 * - `RenderPane` 显示当前标签页对应的视图。
 *
 * 三块之间没有"谁驱动谁"的关系 —— 它们只是**读同一份标签页状态**。
 * 状态本身在 `useTabs()` 里，这里只做**装配**：顶栏菜单、开机那一轮、
 * 监听器的装与摘，以及把三块接到标签页上。
 *
 * 「导航胶水层」（页面之间怎么走）与「快捷键」各自搬去了
 * `composables/useNavigation.ts` / `composables/useShortcuts.ts` —— 它们变更的原因与装配不同，
 * 而标签页状态仍然只有 `useTabs()` 这一份，能力是**递进去**的，不是复制一份。
 */
const {
  tabs,
  activeIndex,
  active,
  committed,
  shakeTick,
  canGoBack,
  canGoForward,
  canReopen,
  select,
  newTab,
  newTabWith,
  close,
  closeOthers,
  reopenClosed,
  move,
  navigate,
  goBack,
  goForward,
} = useTabs();

/**
 * 标题栏绑定的就是当前标签页的地址字段。
 *
 * 用 get/set 而不是直接绑 `active.address`：当前标签页理论上可能为空。
 * 它绑的是**地址栏里的字**（可能在编辑中）；渲染区与标签名只认解析结果。
 */
const address = computed({
  get: () => active.value?.address ?? "",
  set: (value: string) => {
    if (active.value) {
      active.value.address = value;
    }
  },
});

/**
 * 解析失败的原因。
 *
 * 做成整页覆盖而**不是**顶部提示条：地址没解析出来 = 界面根本不知道自己在哪，
 * 这种事不处理就会误事，必须被看到。由当前标签页自己记着，所以切标签页不会串。
 */
const addressError = computed(() => active.value?.error ?? "");

function dismissError() {
  if (active.value) {
    active.value.error = "";
  }
}

/** 地址栏回车 = 导航。解析交给后端，前端不认语法 */
function onSubmit(value: string) {
  void navigate(value, "push");
}

/** 站点名：菜单面板顶上那一行 */
const APP_NAME = "重逢笔记";

/** 顶栏菜单是否展开 */
const menuOpen = ref(false);

/**
 * 菜单要列的页面：**每次打开都问后端**，所以后端加了页面不必重启前端。
 * 特殊页面与帮助页各问一次（帮助是另一个命名空间）。
 */
const specialPages = ref<string[]>([]);
const helpPages = ref<HelpPage[]>([]);

async function onMenu() {
  menuOpen.value = !menuOpen.value;
  if (!menuOpen.value) {
    return;
  }

  try {
    specialPages.value = await invoke<string[]>("special_pages");
  } catch (error) {
    console.warn("取特殊页面清单失败：", error);
    flash(`无法获取页面列表：${error}`);
    specialPages.value = [];
  }

  try {
    helpPages.value = await invoke<HelpPage[]>("help_pages");
  } catch (error) {
    console.warn("取帮助页清单失败：", error);
    helpPages.value = [];
  }
}

/** 菜单里点一条：算一次跳转 */
function openFromMenu(input: string) {
  menuOpen.value = false;
  void navigate(input, "push");
}

/** 在新标签页里打开（Ctrl/Cmd 点击，或条目右键菜单里那一项） */
function openTabWith(input: string) {
  menuOpen.value = false;
  void newTabWith(input);
}

/**
 * 导航胶水层：把上面这些入口接到标签页上。
 *
 * 标签页状态还是 `useTabs()` 那**一份**，这里只是把它要用的几项能力递进去
 * （不复制状态，见 `composables/useNavigation.ts` 的模块头）。
 */
const {
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
  createNote,
  afterUnlock,
  leavePage,
  leaveUnlock,
  afterRollback,
} = useNavigation({ active, committed, canGoBack, navigate, newTabWith, goBack });

/** 关窗前把还没落盘的改动写完。平时靠节流，这一步是兜底 */
function flushOnUnload() {
  void flushPreferences();
}

/** 调试信息框开着没有 */
const debugOpen = ref(false);

/** 滚动容器在渲染区里，所以滚动要它自己做 */
const renderPane = useTemplateRef<{
  scrollToTop: () => void;
  scrollToBottom: () => void;
  /** 打开页内查找；当前页没有正文可查时返回 false（好给一句提示） */
  showFind: () => boolean;
  /** 下一个命中；没有可查的返回 false（好给一句提示） */
  findNext: () => boolean;
  /** 上一个命中 */
  findPrevious: () => boolean;
}>("renderPane");

function toggleDebug() {
  debugOpen.value = !debugOpen.value;
}

function toggleWidth() {
  updatePreferences({ limit_width: !preferences.value.limit_width });
}

/** 修好之后再来一次：回到"正在启动"，加载页会重新出现 */
function retryStartup() {
  restartStartup();
  void openWorkspace();
}

/**
 * 其余快捷键动作：查找、翻页、菜单那些。
 *
 * 标签页那三个由 `useShortcuts` 自己跑（它们就是标签页的事）；这里接的是
 * **别处的动作**，顺带接住将来新加的 —— 所以末尾那个 `default` 是刻意的：
 * 加一个动作忘了在这儿接，表现是"设置页能改、菜单上写着、按了没反应"。
 */
function onShortcut(action: string) {
  switch (action) {
    case "find":
      // 编辑器里有焦点就开**它**的查找面板；没有就开页面上那个。
      //
      // 顺序是有意的：人在编辑器里按 Ctrl+F，想找的多半是编辑器里那段源码
      // （那才是他正在看的东西）。反过来先开页面上的查找的话，
      // 在编辑器里按 Ctrl+F 会在看不见的地方弹出个查找框。
      if (openEditorSearch()) {
        return;
      }
      // 页面上有没有正文可查由当前那一页自己说（`RenderPane` 收集），
      // 这一层不知道现在渲染的是笔记、帮助还是设置页
      if (!renderPane?.value?.showFind()) {
        flash("这一页没有可以查找的正文");
      }
      return;
    case "find-next":
      if (renderPane?.value?.findNext()) {
        return;
      }
      flash("还没开始查找");
      break;
    case "find-previous":
      if (renderPane?.value?.findPrevious()) {
        return;
      }
      flash("还没开始查找");
      break;
    case "back":
      goBack();
      break;
    case "forward":
      goForward();
      break;
    case "home":
      openHome();
      break;
    case "menu":
      void onMenu();
      break;
    case "reload":
      // 整个窗口重载（连后端状态一起重新读一遍）。
      //
      // 而不是 `restartStartup()`：那只是把启动那一轮重跑，界面不重画，
      // 标签页、滚动位置、打开着的编辑器全都还在 —— 与"重载"这两个字给人的预期
      // 不是一回事。真出问题时（界面冻住了、某处状态不对）人要的是彻底重来一次。
      window.location.reload();
      break;
    default:
      console.warn("这个快捷键动作还没接上：", action);
  }
}

/** 全局快捷键。同样只把标签页能力递进去，状态不在这里多一份 */
const { onKeydown } = useShortcuts({ activeIndex, newTab, close, reopenClosed }, onShortcut);

/**
 * 开机那一轮维护：清过期的回收站条目、回收没人引用的内容块。
 *
 * 到期才做（默认每天一次），没到期后端直接返回 `null` —— 所以这里不必自己看时间。
 * 做了什么都写进横幅：这种在背后删东西的事，不能一句话都不说。
 */
async function runMaintenanceOnce() {
  try {
    const report = await invoke<MaintenanceReport | null>("run_maintenance");
    if (!report) {
      return;
    }
    const parts: string[] = [];
    if (report.purged && report.purged.removed > 0) {
      parts.push(`清掉回收站 ${report.purged.removed} 条`);
    }
    if (report.gc && report.gc.removed_blobs > 0) {
      parts.push(
        `回收内容块 ${report.gc.removed_blobs} 个（${formatBytes(report.gc.freed_bytes)}）`,
      );
    }
    if (parts.length > 0) {
      flash(`仓库整理：${parts.join("；")}`);
    }
    await refreshWorkspaceInfo();
  } catch (error) {
    // 维护失败不该挡住开机：记一笔就接着用
    console.warn("自动维护失败：", error);
  }
}

/**
 * 开局的标签页也要有地址。
 *
 * 它在启动跑完之前只是一只空壳（解析要问后端，那时后端还没准备好）；
 * 一旦准备好了就落到 `special:newtab` —— 于是地址栏一开始就写着自己在哪。
 */
/**
 * 启动时先看一件事：**是不是被 `refind://…` 唤起**。
 *
 * 是（冷启动）：那地址就是这一趟的目的地，直接落在**开头那个空标签页**上 ——
 * 此时窗口刚建好，后端把地址寄存在那里等着人来取（见 `take_pending_address`）。
 * 这里**不另开一页**：那会多出一个没人要的"新标签页"要人亲手关掉。
 * （程序**已经在跑**时收到深链是另一回事，那条路开新标签页 —— 见下面那个监听。）
 *
 * 不是：才落到新标签页。两件事都做就成了"先开一页再跳走"，后退键里多一条冤枉路。
 */
async function openStartupAddress() {
  let address: string | null = null;
  try {
    address = await invoke<string | null>("take_pending_address");
  } catch (error) {
    console.warn("取待打开地址失败：", error);
  }

  const tab = active.value;
  if (address) {
    await navigate(address, "push");
  } else if (tab && !tab.address && !tab.route) {
    await navigate("special:newtab", "push");
  }
}

watch(
  startupPhase,
  (phase) => {
    if (phase === "ready") {
      void openStartupAddress();
      void runMaintenanceOnce();
      // 浏览历史读回来一次：那一页要显示它，不必等打开那一页才读
      void loadBrowsing();
    }
  },
  { immediate: true },
);

/** 顶栏那两条深链监听的摘除函数（卸载时要摘掉） */
const unlistenAddress: (() => void)[] = [];

onMounted(() => {
  // 认键是在每次按键时去 `core/keymap.ts` 查的，所以这里的先后无所谓；
  // 读回来之前用的是出厂键位（那也正是"还没读"的正确样子）。
  void loadKeymap();
  window.addEventListener("keydown", onKeydown);
  window.addEventListener("beforeunload", flushOnUnload);
  installWheelZoom();
  // 正文右键里的"在新标签页打开"与 Ctrl+点击走同一个实现
  setOpenInNewTab(openTabWith);
  void interceptClose();
  // 深链来的时候**开一个新标签页**，不动人正在看的这一页：
  // 对方是从别处点了一个链接过来的，不是要你离开手上这一页 ——
  // 直接改当前标签页会把正在读的东西顶掉，而"点了个链接，结果那一页没了"
  // 是最容易让人以为"程序把我的东西弄丢了"的一种。
  //
  // 两条路都汇到这里：
  // - `open-address`：Linux / Windows，系统是"再拉起一次实例、把 URL 当参数给"，
  //   后端把参数解析好发过来（见 `platform::deep_link`）；
  // - `deep-link://new-url`：macOS 与 Android，系统**走事件**把 URL 递给已经在跑的
  //   这一个实例（那是插件发的事件，解析在这儿做 —— 规矩与后端那条一样）。
  void listen<string>("open-address", (event) => {
    openTabWith(event.payload);
  }).then((unlisten) => unlistenAddress.push(unlisten));

  void listen<string[]>("deep-link://new-url", (event) => {
    for (const url of event.payload ?? []) {
      const address = addressFromDeepLink(url);
      if (address) {
        openTabWith(address);
      }
    }
  }).then((unlisten) => unlistenAddress.push(unlisten));
});
onBeforeUnmount(() => {
  window.removeEventListener("keydown", onKeydown);
  window.removeEventListener("beforeunload", flushOnUnload);
  for (const unlisten of unlistenAddress) {
    unlisten();
  }
});

/**
 * 关窗前先把这一趟送出去。
 *
 * 这一层拦的是**所有**关闭的路子：标题栏那颗×、窗口管理器、Alt+F4 ——
 * 它们都走 Tauri 的 `closeRequested`，拦一处就够。
 *
 * 拿不到锁、网断了也照关：东西在本机，下次同步还在。真等不下去时，
 * 遮罩上那颗"不等了"是**直接销毁窗口**（正在跑的那一趟只能由它被中断）。
 */
async function interceptClose() {
  // 手机上没窗口可拦：`onCloseRequested` 会抛 `plugin windows not initialized`。
  // 判据是 `isMobile()` 而不是"拿到对象没有" —— 后者在移动端照样给得出对象。
  if (isMobile()) {
    return;
  }
  const appWindow = currentWindow();
  if (!appWindow) {
    return;
  }
  await appWindow.onCloseRequested(async (event) => {
    event.preventDefault();
    await syncBeforeClose();
    await appWindow.destroy();
  });
}
</script>

<template>
  <!-- 拖边框调窗口大小：手机上整个概念不存在（窗口由系统管），连热区都不该画出来 -->
    <WindowResizeHandles v-if="!isMobile()"/>

  <div class="app">
    <WindowTitleBar
        v-model="address"
        :committed="committed"
        :ready="startupPhase === 'ready'"
        :can-back="canGoBack"
        :can-forward="canGoForward"
        @back="goBack"
        @forward="goForward"
        @home="openHome"
        @menu="onMenu"
        @submit="onSubmit"
    />

    <div class="app__main">
      <!-- 启动的三种状态在主区域上是三选一：还在跑 / 跑完 / 没跑完。
           标题栏始终留着 —— 窗口一直拖得动、关得掉 -->
      <StartupLoading v-if="startupPhase === 'starting'"/>
      <StartupError v-else-if="startupPhase === 'failed'" @retry="retryStartup"/>

      <template v-else>
        <TabRail
            :tabs="tabs"
            :active="activeIndex"
            :shake-tick="shakeTick"
            :can-reopen="canReopen"
            @select="select"
            @close="close"
            @close-others="closeOthers"
            @reopen="reopenClosed"
            @new-tab="newTab"
            @move="move"
            @settings="openSpecial('settings')"
        />

        <RenderPane
            ref="renderPane"
            :tab="active"
            @navigate="openNote"
            @redirect="redirectNote"
            @navigate-new-tab="openNoteInNewTab"
            @section="openSection"
            @edit-navigate="leaveEditor"
            @open-version="openVersion"
            @rollback-version="rollbackVersion"
            @deleted="afterDelete"
            @create-note="createNote"
            @rolled-back="afterRollback"
            @unlocked="afterUnlock"
            @unlock-cancel="leaveUnlock"
            @leave="leavePage"
        />
      </template>
    </div>
  </div>

  <!-- 右下角：调试信息框在那组按钮**上方**，所以两者装进同一个容器，
       不必手算按钮堆了多高。启动跑完之前整组收起（那时渲染区还不是真东西） -->
  <div v-if="startupPhase === 'ready'" class="corner">
    <DebugPanel v-if="debugOpen" :tab="active" @close="debugOpen = false"/>
    <FloatingTools
        :limited="preferences.limit_width"
        :theme="preferences.theme"
        :debug-open="debugOpen"
        @toggle-debug="toggleDebug"
        @toggle-theme="cycleTheme"
        @toggle-width="toggleWidth"
        @scroll-top="renderPane?.scrollToTop()"
        @scroll-bottom="renderPane?.scrollToBottom()"
        @find="onShortcut('find')"
    />
  </div>

  <!-- 关窗前的同步：摆一块遮罩，别让人对着没反应的窗口再点一次 -->
  <div v-if="syncClosing" class="closing-sync" role="status">
    <div class="closing-sync__box">
      <span class="closing-sync__spinner" aria-hidden="true"/>
      <p class="closing-sync__text">
        {{ syncProgress ? `正在同步：${syncProgress.text}` : "正在同步…" }}
      </p>
      <button class="closing-sync__skip" type="button" @click="currentWindow()?.destroy()">
        不等了，直接关闭
      </button>
      <p class="closing-sync__hint">没传完的东西留在本机，下次同步会接着传。</p>
    </div>
  </div>

  <ContextMenu/>

  <!-- 顶栏菜单：始终挂载（只有 open 变），出现与收起才各自播得完动画 -->
  <AppMenu
      :open="menuOpen"
      :pages="specialPages"
      :help="helpPages"
      :title="APP_NAME"
      @open="openFromMenu"
      @open-new-tab="openTabWith"
      @close="menuOpen = false"
      @home="openHome"
      @sync="syncNowAndReport"
  />

  <!-- 解析失败：整页覆盖（点空白处或点按钮关掉） -->
  <div v-if="addressError" class="error-cover" @click.self="dismissError">
    <p class="notice notice--error">
      <span>{{ addressError }}</span>
      <button type="button" @click="dismissError">关闭</button>
    </p>
  </div>

  <!-- 一条实话：顶部悬挂条（只是提醒，不挡渲染区），自己会散掉 -->
  <div v-if="notice" class="hint-bar" role="status">
    <span>{{ notice }}</span>
    <button type="button" @click="dismissNotice">知道了</button>
  </div>
</template>

<style scoped>
.app {
  display: flex;
  flex-direction: column;
  height: 100vh;
}

/*
 * min-height: 0 是必须的：否则这个 flex 项会被内容撑开，
 * 里面渲染区的 overflow 就再也滚不动了。
 */
.app__main {
  display: flex;
  flex: 1 1 auto;
  min-height: 0;
}

/*
 * 窗口窄了：主区域改竖排，标签栏（它自己会 `order: 2`）落到正文下面变成一条横栏。
 * 断点 760px 与 `TabRail.vue` 里那条是**同一个数** —— 两处一起改。
 */
@media (max-width: 760px) {
  .app__main {
    flex-direction: column;
  }
}

/* 关窗前的同步：盖住整窗，但留一条"不等了"的路 */
.closing-sync {
  position: fixed;
  inset: 0;
  z-index: 90;
  display: flex;
  align-items: center;
  justify-content: center;
  background: color-mix(in srgb, var(--bg) 78%, transparent);
}

.closing-sync__box {
  display: flex;
  flex-direction: column;
  gap: 10px;
  align-items: center;
  max-width: 420px;
  padding: 20px 24px;
  border: 1px solid var(--border);
  border-radius: 10px;
  background: var(--surface);
  text-align: center;
}

.closing-sync__spinner {
  width: 18px;
  height: 18px;
  border: 2px solid var(--border);
  border-top-color: var(--accent);
  border-radius: 50%;
  animation: closing-sync-spin 700ms linear infinite;
}

@keyframes closing-sync-spin {
  to {
    transform: rotate(360deg);
  }
}

.closing-sync__text {
  margin: 0;
  color: var(--text);
  font-size: 13.5px;
}

.closing-sync__skip {
  padding: 5px 12px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: transparent;
  color: var(--text-dim);
  font: inherit;
  font-size: 12.5px;
  cursor: pointer;
}

.closing-sync__skip:hover {
  border-color: var(--accent-soft);
  color: var(--text);
}

.closing-sync__hint {
  margin: 0;
  color: var(--text-dim);
  font-size: 12px;
}

/* 右下角那一堆：按钮与它上方的调试信息框 */
.corner {
  position: fixed;
  right: 16px;
  /*
   * 底部要避开两样东西：**系统栏**与**底部标签栏**。
   *
   * 桌面浏览器与宽窗口下两者都是 0（标签栏在左边、不占底部），于是 `max()` 落到
   * 28px —— 那是这个位置原来的值，桌面观感不变。
   *
   * 手机（窄窗口）下标签栏横到了正文下方占 44px（`rail.css` 里那条 media 赋值），
   * 不避开它的话这一列就压在标签栏上：截图里最后那颗「↓` 正好落在标签栏那一行。
   * 而系统栏那边，沉浸式下 `env()` 通常是 0，但不保证（某些 ROM 仍会报真实值），
   * 所以两条路都留着。
   *
   * 取三者里**最大的那个**：谁占地方就躲开谁。
   */
  bottom: max(28px, var(--rail-bottom-height), calc(var(--safe-bottom) + 8px));
  right: max(16px, calc(var(--safe-right) + 8px));
  z-index: 30;
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  gap: 10px;
}

.hint-bar {
  position: fixed;
  top: calc(var(--titlebar-height) + 10px);
  left: 50%;
  z-index: 46;
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
  align-items: center;
  max-width: min(720px, 88vw);
  padding: 9px 14px;
  border: 1px solid var(--accent-soft);
  border-left-width: 3px;
  border-radius: 8px;
  background: var(--surface);
  color: var(--text-dim);
  font-size: 13px;
  box-shadow: 0 10px 30px rgb(0 0 0 / 25%);
  transform: translateX(-50%);
}

.hint-bar button {
  appearance: none;
  height: 26px;
  padding: 0 10px;
  border: 1px solid var(--accent-soft);
  border-radius: 6px;
  background: transparent;
  color: var(--accent-soft);
  font-size: 12.5px;
  cursor: pointer;
}

.hint-bar button:hover {
  background: var(--hover);
}

/* 整页遮罩 + 居中卡片：错误必须被看到 */
.error-cover {
  position: fixed;
  inset: 0;
  z-index: 55;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 24px;
  background: rgb(0 0 0 / 45%);
}

.notice {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
  align-items: center;
  max-width: min(520px, 86vw);
  margin: 0;
  padding: 14px 18px;
  border: 1px solid var(--border);
  border-radius: 10px;
  background: var(--surface);
  color: var(--text);
  font-size: 13px;
  line-height: 1.6;
  box-shadow: 0 20px 60px rgb(0 0 0 / 35%);
}

.notice--error {
  border-color: var(--danger);
}

.notice button {
  appearance: none;
  height: 26px;
  padding: 0 10px;
  border: 1px solid var(--accent-soft);
  border-radius: 6px;
  background: transparent;
  color: var(--accent-soft);
  font-size: 12.5px;
  cursor: pointer;
}

.notice button:hover {
  background: var(--hover);
}
</style>
