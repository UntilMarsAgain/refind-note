<script setup lang="ts">
import { onMounted, onUnmounted, ref } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Copy, Menu, Minus, Search, Square, X } from "@lucide/vue";

/**
 * 自绘标题栏。
 *
 * 中间是一个「地址栏式」输入框：静止时显示窗口标题，聚焦后可编辑。
 * 依赖 tauri.conf.json 的 `decorations: false`，以及 capabilities 里的
 * core:window:allow-{minimize,toggle-maximize,close,title,start-dragging}。
 */

const emit = defineEmits<{
  (e: "search"): void;
  (e: "menu"): void;
  (e: "submit", value: string): void;
}>();

const appWindow = getCurrentWindow();
const isMaximized = ref(false);

const fieldEl = ref<HTMLInputElement | null>(null);
/** 静止时是窗口标题，聚焦后可编辑 */
const draft = ref("");
/** 最近一次提交的值，Esc 或失焦时回退到它 */
let committed = "";

let unlistenResized: (() => void) | undefined;

onMounted(async () => {
  committed = await appWindow.title();
  draft.value = committed;

  isMaximized.value = await appWindow.isMaximized();
  // 最大化状态会被拖动、双击、系统快捷键改变，必须跟着事件走，
  // 否则「最大化 / 还原」的图标会停在错误的那一个上。
  unlistenResized = await appWindow.onResized(async () => {
    isMaximized.value = await appWindow.isMaximized();
  });
});

onUnmounted(() => unlistenResized?.());

function onFocus() {
  // 地址栏惯例：一点就全选
  fieldEl.value?.select();
}

function onKeydown(event: KeyboardEvent) {
  if (event.key === "Enter") {
    committed = draft.value;
    emit("submit", draft.value);
    fieldEl.value?.blur();
  } else if (event.key === "Escape") {
    draft.value = committed;
    fieldEl.value?.blur();
  }
}

function onBlur() {
  // 没提交就退回上一次的值，行为对齐浏览器地址栏
  if (draft.value !== committed) {
    draft.value = committed;
  }
}
</script>

<template>
  <header class="titlebar" data-tauri-drag-region>
    <div class="titlebar__start">
      <!-- 先放占位图标，之后再换成真正的应用图标 -->
      <img
        class="logo"
        src="/tauri.svg"
        alt=""
        draggable="false"
        data-tauri-drag-region
      />
      <span class="divider" data-tauri-drag-region />

      <button
        class="tbtn"
        type="button"
        title="搜索"
        aria-label="搜索"
        @click="emit('search')"
      >
        <Search :size="16" :stroke-width="1.75" />
      </button>
      <button
        class="tbtn"
        type="button"
        title="菜单"
        aria-label="菜单"
        @click="emit('menu')"
      >
        <Menu :size="16" :stroke-width="1.75" />
      </button>
    </div>

    <div class="titlebar__center">
      <input
        ref="fieldEl"
        v-model="draft"
        class="field"
        type="text"
        spellcheck="false"
        :title="draft"
        @focus="onFocus"
        @keydown="onKeydown"
        @blur="onBlur"
      />
    </div>

    <div class="titlebar__controls">
      <button
        class="wbtn"
        type="button"
        title="最小化"
        aria-label="最小化"
        @click="appWindow.minimize()"
      >
        <Minus :size="14" />
      </button>
      <button
        class="wbtn"
        type="button"
        :title="isMaximized ? '向下还原' : '最大化'"
        :aria-label="isMaximized ? '向下还原' : '最大化'"
        @click="appWindow.toggleMaximize()"
      >
        <Copy v-if="isMaximized" :size="12" />
        <Square v-else :size="12" />
      </button>
      <button
        class="wbtn wbtn--close"
        type="button"
        title="关闭"
        aria-label="关闭"
        @click="appWindow.close()"
      >
        <X :size="14" />
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
  background: var(--titlebar-bg);
}

.titlebar__start {
  display: flex;
  align-items: center;
  gap: 2px;
  padding-left: 8px;
}

.logo {
  display: block;
  width: 18px;
  height: 18px;
  margin: 0 2px;
}

.divider {
  width: 1px;
  height: 16px;
  margin: 0 8px;
  background: rgba(255, 255, 255, 0.3);
}

.titlebar__center {
  display: flex;
  flex: 1 1 auto;
  align-items: center;
  justify-content: center;
  min-width: 0;
  padding: 0 10px;
}

/* 静止时看起来就是一行窗口标题；聚焦后才显出输入框的样子 */
.field {
  width: min(100%, 520px);
  height: 24px;
  padding: 0 10px;
  border: 1px solid transparent;
  border-radius: 6px;
  background: transparent;
  color: var(--text);
  font: inherit;
  font-size: 12.5px;
  text-align: center;
  text-overflow: ellipsis;
  outline: none;
  cursor: default;
  transition: background-color 120ms ease, border-color 120ms ease;
}

.field:hover {
  background: var(--hover);
}

.field:focus {
  background: rgba(0, 0, 0, 0.45);
  border-color: var(--border);
  cursor: text;
}

.titlebar__controls {
  display: flex;
  align-self: stretch;
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

.tbtn:hover {
  background: var(--hover);
}

.tbtn:active {
  background: var(--press);
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
</style>
