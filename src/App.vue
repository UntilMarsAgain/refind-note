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
import ImageViewer from "./components/shell/ImageViewer.vue";
import RenderPane from "./components/shell/RenderPane.vue";
import StartupError from "./components/shell/StartupError.vue";
import StartupLoading from "./components/shell/StartupLoading.vue";
import TabRail from "./components/shell/TabRail.vue";
import WindowResizeHandles from "./components/shell/WindowResizeHandles.vue";
import WindowTitleBar from "./components/shell/WindowTitleBar.vue";
import { listen } from "@tauri-apps/api/event";
import { withSection } from "./core/address.ts";
import { addressFromDeepLink } from "./core/deep-link.ts";
import type { HelpPage } from "./ipc/help.ts";
import { loadBrowsing } from "./core/browsing.ts";
import { dismissNotice, flash, notice } from "./core/notice.ts";
import { syncBeforeClose, syncClosing, syncProgress } from "./core/sync.ts";
import { currentWindow } from "./core/window-api.ts";
import { setOpenInNewTab } from "./dom/note-html.ts";
import {
  cycleTheme,
  flushPreferences,
  openWorkspace,
  preferences,
  refreshWorkspaceInfo,
  updatePreferences,
} from "./core/preferences.ts";
import { restartStartup, startupPhase } from "./core/startup.ts";
import { formatBytes, type MaintenanceReport } from "./ipc/maintenance.ts";
import { useTabs } from "./core/tabs.ts";
import { installWheelZoom } from "./dom/zoom-wheel.ts";

/**
 * 窗口骨架。
 *
 * - `WindowTitleBar` 绘制标题栏，与「当前标签页的地址」双向绑定；
 * - `TabRail` 绘制垂直标签页；
 * - `RenderPane` 显示当前标签页对应的视图。
 *
 * 三块之间没有"谁驱动谁"的关系 —— 它们只是**读同一份标签页状态**。
 * 状态本身在 `useTabs()` 里，这里只做组装、导航入口与键盘快捷键。
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

/** 标题栏那颗首页按钮：回新标签页（它也是一个地址，走同一条导航） */
function openHome() {
  void navigate("special:newtab", "push");
}

/** 标签栏底下的入口：它也是地址（`special:xxx`），算一次跳转 */
function openSpecial(page: string) {
  void navigate(`special:${page}`, "push");
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
 * 从解锁页退出来。
 *
 * 进这一页时那条记录已经被**替换**掉了（原来那一页读不出来，留着它只会再被送回来），
 * 所以"退一步"就是后退：回到进来之前待的地方。一整个标签页都是从 `@unlock` 开局的，
 * 没处可退，就摆到全部页面上让人挑。
 */
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
    newTab();
    return;
  }
  if (key === "w") {
    event.preventDefault();
    close(activeIndex.value);
  }
}

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
  <WindowResizeHandles/>

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

  <ImageViewer/>

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
  bottom: 28px;
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
