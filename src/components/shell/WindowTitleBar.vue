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
import { syncAvailable, syncBusy, syncNow, syncProgress } from "../../core/sync.ts";
import { describeReport } from "../../ipc/sync.ts";
import { flash } from "../../core/notice.ts";
import { currentWindow } from "../../core/window-api.ts";

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

const appWindow = currentWindow();
const isMaximized = ref(false);
let unlistenResized: (() => void) | undefined;
onMounted(async () => {
  if (!appWindow) {
    return;
  }
  isMaximized.value = await appWindow.isMaximized();
  // 最大化状态会被拖动、双击、系统快捷键改变，必须跟着事件走，
  // 否则「最大化 / 还原」的图标会停在错误的那一个上
  unlistenResized = await appWindow.onResized(async () => {
    isMaximized.value = await appWindow.isMaximized();
  });
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
async function runSync() {
  try {
    const report = await syncNow();
    if (report) {
      flash(`同步完成：${describeReport(report)}`);
    }
  } catch (error) {
    // 锁被别的机器拿着之类：说清楚，别让人对着没反应的按钮猜
    flash(`同步没成功：${error}`);
  }
}

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
          @click="runSync"
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

    <div class="titlebar__controls">
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
  height: var(--titlebar-height);
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
  flex: 1 1 auto;
  align-items: center;
  justify-content: center;
  min-width: 0;
  padding: 0 10px;
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
