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
import { computed, onMounted, onUnmounted, ref } from "vue";
import {
  ArrowLeft,
  ArrowRight,
  CloudUpload,
  Copy,
  House,
  Menu,
  Minus,
  Square,
  X,
} from "@lucide/vue";
import { logoSrc } from "../../core/theme.ts";
import { syncAvailable, syncBusy, syncNowAndReport, syncProgress } from "../../core/sync.ts";
import { currentWindow } from "../../core/window-api.ts";
import { isMobile } from "../../core/platform.ts";

/**
 * 自绘标题栏。
 *
 * 中间是一个「地址栏式」输入框：静止时看起来就是一行窗口标题，聚焦后可编辑。
 * 依赖 tauri.conf.json 的 `decorations: false`，以及 capabilities 里的
 * core:window:allow-{minimize,toggle-maximize,close,start-dragging}。
 */
const props = defineProps<{
  /** 规范地址（当前标签页解析结果里那一份）。失焦 / Esc 以它回显 */
  committed: string;
  /** 启动跑完了没有：没跑完之前不显示地址栏与菜单（那时界面还不是真东西） */
  ready?: boolean;
  canBack?: boolean;
  canForward?: boolean;
}>();

const address = defineModel<string>({ required: true });

const emit = defineEmits<{
  (e: "back"): void;
  (e: "forward"): void;
  /** 回新标签页：从哪儿都回得去的那一个地方 */
  (e: "home"): void;
  (e: "menu"): void;
  /** 用户按下回车：交给持有标签页的一方去解析、导航 */
  (e: "submit", value: string): void;
}>();

/**
 * 窗口对象 —— **手机上根本没有**。
 *
 * `currentWindow()` 自己不抛（它 try/catch 了），但返回的那个对象**的方法会抛**：
 * `isMaximized()` 走到后端就报 `plugin windows not initialized`，
 * 那条错一路冒到 `app.config.errorHandler`，于是启动时弹出"界面出了点问题"。
 *
 * 所以判据要用 `isMobile()`（编译期就定的），而不是"拿到对象没有" ——
 * 后者在移动端**照样给得出对象**，只是用不了。
 */
const appWindow = isMobile() ? null : currentWindow();
const isMaximized = ref(false);
let unlistenResized: (() => void) | undefined;
onMounted(async () => {
  if (!appWindow) {
    return;
  }
  // 整个 try/catch 是兜底：桌面端万一在窗口还没建好时问，也别把启动搞崩
  try {
    isMaximized.value = await appWindow.isMaximized();
    // 最大化状态会被拖动、双击、系统快捷键改变，必须跟着事件走，
    // 否则「最大化 / 还原」的图标会停在错误的那一个上
    unlistenResized = await appWindow.onResized(async () => {
      isMaximized.value = await appWindow.isMaximized();
    });
  } catch (error) {
    console.warn("问窗口状态失败（图标可能停在错的那个上）：", error);
  }
});
onUnmounted(() => unlistenResized?.());

/** 同步按钮上那句话：正在跑就报走到哪儿了，没跑就说"立即同步" */
const syncTitle = computed(() => {
  if (!syncBusy.value) {
    return "立即同步";
  }
  return syncProgress.value ? `正在同步：${syncProgress.value.text}` : "正在同步…";
});

/** 点一下：立刻同步一次（不等冷却也不等落定），完事说一句做了什么 */

const fieldEl = ref<HTMLInputElement | null>(null);

function onFocus() {
  // 地址栏惯例：一点就全选
  fieldEl.value?.select();
}

function onKeydown(event: KeyboardEvent) {
  if (event.key === "Enter") {
    emit("submit", address.value);
    // 刻意**不 blur**：提交后光标留在地址栏，方便接着敲下一条。
    // 回显（规范地址）由父组件写回来，这里不碰焦点。
    return;
  }
  if (event.key === "Escape") {
    // 放弃这次编辑：退回规范地址
    address.value = props.committed;
    fieldEl.value?.blur();
  }
}

// 失焦一律以**规范地址**回显 —— 无论刚才有没有导航成功。
function onBlur() {
  address.value = props.committed;
}
</script>

<template>
  <!-- "deep" = 子树内任意位置都能拖动。
       输入框与按钮属于 Tauri 的 CLICKABLE_TAGS，会自动阻断拖动，所以不必逐个排除。 -->
  <header class="titlebar" data-tauri-drag-region="deep">
    <!--
      左：logo + 后退（+ 桌面专属的首页/同步）。

      手机上去掉 logo、也去掉首页与同步：顶栏本来就窄，而正文里已经有一个
      "新标签页"入口、菜单浮层里也有那几个页面 —— 手机上要的是**把常用的
      留在外面**（后退/前进/地址/菜单），不是把桌面那一排原样搬过来。

      ⚠️ 下面这两段是**各自独立的两套**：`__mobile` 那一整段只在手机上渲染。
      桌面端必须**保持原样** —— 所以桌面那一排的按钮顺序、位置一个都没动，
      新的排布不套用到它身上。（我第一版是给两边共用一套结构，结果把桌面端
      的按钮顺序也改了，那是回归。）
    -->
    <div class="titlebar__start">
      <img class="logo" :src="logoSrc" alt="重逢笔记" draggable="false"/>
      <span class="divider"/>

      <button
          class="tbtn"
          type="button"
          aria-label="后退"
          :disabled="!canBack"
          @click="emit('back')"
      >
        <ArrowLeft :size="16" :stroke-width="1.75"/>
      </button>

      <button
          class="tbtn"
          type="button"
          aria-label="前进"
          :disabled="!canForward"
          @click="emit('forward')"
      >
        <ArrowRight :size="16" :stroke-width="1.75"/>
      </button>

      <button
          v-if="ready !== false"
          class="tbtn"
          type="button"
          aria-label="首页"
          title="首页（新标签页）"
          @click="emit('home')"
      >
        <House :size="16" :stroke-width="1.75"/>
      </button>

      <!-- 同步：没配同步就不摆它（摆了也只是点一下报"没开"） -->
      <button
          v-if="ready !== false && syncAvailable"
          class="tbtn"
          type="button"
          aria-label="立即同步"
          :title="syncTitle"
          :disabled="syncBusy"
          @click="syncNowAndReport"
      >
        <CloudUpload :size="16" :stroke-width="1.75" :class="{ 'tbtn--spinning': syncBusy }"/>
      </button>

      <button
          v-if="ready !== false"
          class="tbtn"
          type="button"
          aria-label="菜单"
          @click="emit('menu')"
      >
        <Menu :size="16" :stroke-width="1.75"/>
      </button>
    </div>

    <!--
      中：地址栏。

      桌面端这一段的**宽度行为与原来一样**（`flex: 1`，见 `.titlebar__center`），
      所以桌面上的观感不变 —— 那一版"地址栏不在正中"是桌面本来的样子，不动它。
      手机上左右配平之后它正好落在中间。
    -->
    <div v-if="ready !== false" class="titlebar__center">
      <input
          ref="fieldEl"
          v-model="address"
          class="field"
          type="text"
          spellcheck="false"
          placeholder="请输入地址"
          :title="address"
          @focus="onFocus"
          @keydown="onKeydown"
          @blur="onBlur"
      />
    </div>
    <div v-else class="titlebar__center"/>

    <!--
      右边那一段**只在手机上渲染**：放「前进」与「菜单」。

      收进菜单的理由不是"地方不够"，而是它们在手机上**用得少**
      （首页有新产品页、同步是偶尔的事），而前进/后退是天天按的。
      所以按"常用的留在外面"分。
    -->
    <div v-if="isMobile()" class="titlebar__end">
      <button
          class="tbtn"
          type="button"
          aria-label="前进"
          :disabled="!canForward"
          @click="emit('forward')"
      >
        <ArrowRight :size="16" :stroke-width="1.75"/>
      </button>

      <button
          v-if="ready !== false"
          class="tbtn"
          type="button"
          aria-label="菜单"
          @click="emit('menu')"
      >
        <Menu :size="16" :stroke-width="1.75"/>
      </button>
    </div>

    <!--
      窗口操作（最小化 / 最大化 / 关闭）**只在桌面显示**。

      手机上根本没有这些概念：系统管多任务与应用切换，不给应用最小化的接口
      （`minimize()` 在 Android 上无效），最大化更是无从谈起。摆出来只会让人
      点一个没反应的按钮 —— 而"点了没反应"比"没有这个按钮"糟得多。

      判据用 `isMobile()`：它问的是**后端**（`platform_kind`），那个值编译期就定了，
      不像前端嗅 UA 那样把桌面浏览器和手机混起来。
    -->
    <div v-if="!isMobile()" class="titlebar__controls">
      <button
          class="wbtn"
          type="button"
          aria-label="最小化"
          @click="appWindow?.minimize()"
      >
        <Minus :size="14"/>
      </button>
      <button
          class="wbtn"
          type="button"
          :aria-label="isMaximized ? '向下还原' : '最大化'"
          @click="appWindow?.toggleMaximize()"
      >
        <Copy v-if="isMaximized" :size="12"/>
        <Square v-else :size="12"/>
      </button>
      <button
          class="wbtn wbtn--close"
          type="button"
          aria-label="关闭"
          @click="appWindow?.close()"
      >
        <X :size="14"/>
      </button>
    </div>
  </header>
</template>

<style scoped>
.titlebar {
  display: flex;
  align-items: center;
  flex: 0 0 auto;
  /*
   * 高度要把状态栏**让出来**。
   *
   * 手机上 WebView 铺到整块屏幕，这一栏自己画在 `top: 0`，于是上半截（连同 logo 与
   * 地址框）藏在状态栏底下 —— 表现是"标题栏被遮住一半"。
   *
   * 用 `padding-top` 而不是 `margin-top`：让出来的那一块要**仍然是标题栏的背景**，
   * 否则状态栏那一段会露出底下页面的颜色，看起来像标题栏中间开了个洞。
   * 桌面浏览器上 `env(safe-area-inset-top)` 是 0，这一行等于什么都不做。
   */
  height: calc(var(--titlebar-height) + var(--safe-top));
  padding-top: var(--safe-top);
  box-sizing: border-box;
  /* 与正文同一个变量，两者之间不留分界 */
  background: var(--bg);
  transition: background-color 160ms ease;
  color: var(--text);
  user-select: none;
  -webkit-user-select: none;
}

.titlebar__start {
  display: flex;
  align-items: center;
  gap: 2px;
  padding-left: 8px;
}

.logo {
  display: block;
  width: 20px;
  height: 20px;
  margin: 0 2px;
  -webkit-user-drag: none;
}

.divider {
  width: 1px;
  height: 18px;
  margin: 0 8px;
  background: var(--divider);
}

.titlebar__center {
  display: flex;
  /*
   * `flex: 1 1 auto` —— **桌面端这一条没动过**，维持它原本的观感。
   *
   * 我曾经把它改成"三段各按内容定宽、这段独占剩余"以求地址栏真居中，
   * 结果桌面上的按钮顺序与位置也跟着变了。那是回归：要求是"桌面保持不变，
   * 手机用新排布"，不是"两边都换成新的"。
   *
   * 手机上靠的是另一件事 —— 左右各只留一颗按钮、宽度相当，于是这段
   * 自然落在中间（见模板里 `isMobile()` 那一段）。
   */
  flex: 1 1 auto;
  align-items: center;
  justify-content: center;
  min-width: 0;
  padding: 0 10px;
}

/* 右侧（手机上才渲染）：前进 + 菜单，按内容定宽 */
.titlebar__end {
  display: flex;
  align-items: center;
  gap: 2px;
  padding-right: 8px;
}

/* 静止时看起来就是一行窗口标题；聚焦后才显出输入框的样子。
   尺寸对齐 Chrome：栏高 40px / 地址栏 28px / 文字 14px */
.field {
  width: min(100%, 520px);
  height: 28px;
  padding: 0 10px;
  border: 1px solid transparent;
  border-radius: 6px;
  background: transparent;
  color: var(--text);
  font: inherit;
  font-size: 14px;
  text-align: center;
  text-overflow: ellipsis;
  outline: none;
  cursor: default;
  transition: background-color 120ms ease, border-color 120ms ease;
}

.field::placeholder {
  color: var(--text-dim);
}

.field:hover {
  background: var(--hover);
}

.field:focus {
  background: var(--field-bg);
  border-color: var(--border);
  cursor: text;
}

.titlebar__controls {
  display: flex;
  align-self: stretch;
}

/* 直接挂在标题栏上的那颗按钮（同步）：与窗口按钮之间留一条缝 ——
   紧挨着最小化，容易被当成"又一个窗口按钮"。
   （原先这条缝挂在主题按钮上，那颗挪去悬浮工具了） */
.titlebar > .tbtn {
  margin-right: 6px;
}

.tbtn {
  appearance: none;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  padding: 0;
  border: 0;
  border-radius: 5px;
  background: transparent;
  color: var(--icon);
  cursor: default;
}

.tbtn:hover:not(:disabled) {
  background: var(--hover);
}

.tbtn:active:not(:disabled) {
  background: var(--press);
}

.tbtn:disabled {
  opacity: 0.35;
}

/* 正在同步：那只云转起来（"在动"比"变灰"更能说明它没坏） */
.tbtn--spinning {
  animation: tbtn-spin 1.4s linear infinite;
}

@keyframes tbtn-spin {
  to {
    transform: rotate(360deg);
  }
}

@media (prefers-reduced-motion: reduce) {
  .tbtn--spinning {
    animation: none;
  }
}

.wbtn {
  appearance: none;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 44px;
  height: 100%;
  padding: 0;
  border: 0;
  background: transparent;
  color: var(--icon);
  cursor: default;
  transition: background-color 120ms ease;
}

.wbtn:hover {
  background: var(--hover);
}

.wbtn:active {
  background: var(--press);
}

.wbtn--close:hover,
.wbtn--close:active {
  background: var(--accent);
  color: #fff;
}

@media (prefers-reduced-motion: reduce) {
  .titlebar,
  .field,
  .wbtn {
    transition: none;
  }
}
</style>
