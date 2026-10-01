<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, useTemplateRef, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import AppMenu from "./components/shell/AppMenu.vue";
import ContextMenu from "./components/shell/ContextMenu.vue";
import DebugPanel from "./components/shell/DebugPanel.vue";
import FloatingTools from "./components/shell/FloatingTools.vue";
import ImageViewer from "./components/shell/ImageViewer.vue";
import RenderPane from "./components/RenderPane.vue";
import StartupError from "./components/shell/StartupError.vue";
import StartupLoading from "./components/shell/StartupLoading.vue";
import TabRail from "./components/shell/TabRail.vue";
import WindowResizeHandles from "./components/shell/WindowResizeHandles.vue";
import WindowTitleBar from "./components/shell/WindowTitleBar.vue";
import { withSection } from "./core/address.ts";
import { loadBrowsing } from "./core/browsing.ts";
import { dismissNotice, flash, notice } from "./core/notice.ts";
import { setOpenInNewTab } from "./view/note-html.ts";
import {
  cycleTheme,
  flushPreferences,
  openWorkspace,
  preferences,
  refreshWorkspaceInfo,
  updatePreferences,
} from "./core/preferences.ts";
import { restartStartup, startupPhase } from "./core/startup.ts";
import { formatBytes, type MaintenanceReport } from "./bindings/maintenance.ts";
import { useTabs } from "./core/tabs.ts";
import { installWheelZoom } from "./view/zoom-wheel.ts";

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

/** 标签栏底下的入口：它也是地址（`special:xxx`），算一次跳转 */
function openSpecial(page: string) {
  void navigate(`special:${page}`, "push");
}

/** 站点名：菜单面板顶上那一行 */
const APP_NAME = "重逢笔记";

/** 顶栏菜单是否展开 */
const menuOpen = ref(false);

/** 菜单要列的页面：**每次打开都问后端**，所以后端加了页面不必重启前端 */
const specialPages = ref<string[]>([]);

async function onMenu() {
  menuOpen.value = !menuOpen.value;
  if (!menuOpen.value) {
    return;
  }

  try {
    specialPages.value = await invoke<string[]>("special_pages");
  } catch (error) {
    console.warn("取特殊页面清单失败：", error);
    flash(`取不到特殊页面清单：${error}`);
    specialPages.value = [];
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
 * 某一页解开了锁：**退回上一条地址**。
 *
 * 人是从"某一页（或某一版）读不出来"走到 `@unlock` 的，解开之后回到原地最省事 ——
 * 那条地址现在读得出来了。
 */
function afterUnlock() {
  if (canGoBack.value) {
    void goBack();
    return;
  }
  const title = currentNoteTitle();
  if (title) {
    void navigate(title, "push");
  }
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
  flash(`已回滚：第 ${rev} 版保存的是旧内容，原历史一条未动`);
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
      flash(`仓库整理：${parts.join("、")}`);
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
watch(
  startupPhase,
  (phase) => {
    const tab = active.value;
    if (phase === "ready") {
      if (tab && !tab.address && !tab.route) {
        void navigate("special:newtab", "push");
      }
      void runMaintenanceOnce();
      // 浏览历史读回来一次：那一页要显示它，不必等打开那一页才读
      void loadBrowsing();
    }
  },
  { immediate: true },
);

onMounted(() => {
  window.addEventListener("keydown", onKeydown);
  window.addEventListener("beforeunload", flushOnUnload);
  installWheelZoom();
  // 正文右键里的"在新标签页打开"与 Ctrl+点击走同一个实现
  setOpenInNewTab(openTabWith);
});
onBeforeUnmount(() => {
  window.removeEventListener("keydown", onKeydown);
  window.removeEventListener("beforeunload", flushOnUnload);
});
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
        @menu="onMenu"
        @theme="cycleTheme"
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

  <ImageViewer/>

  <ContextMenu/>

  <!-- 顶栏菜单：始终挂载（只有 open 变），出现与收起才各自播得完动画 -->
  <AppMenu
      :open="menuOpen"
      :pages="specialPages"
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
