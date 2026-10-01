<script setup lang="ts">
import {logoSrc} from "../theme.ts";
import {ArrowLeft, ArrowRight, Menu, Minus, Copy, Square, X} from "@lucide/vue";
import {currentWindow} from "../window-api.ts";
import {onMounted, onUnmounted, ref} from "vue";

const props = defineProps<{
  /** 规范地址（当前标签页解析结果里那一份）。失焦 / Esc 以它回显 */
  committed: string;
  canBack?: boolean;
  canForward?: boolean;
}>();
const address = defineModel<string>({required: true});

const emit = defineEmits<{
  (e: "back"): void;
  (e: "forward"): void;
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
  // 绑定系统的窗口最大化状态
  unlistenResized = await appWindow.onResized(async () => {
    isMaximized.value = await appWindow.isMaximized();
  });
});
onUnmounted(() => unlistenResized?.());

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
  <div class="titlebar" data-tauri-drag-region="deep">
    <div class="titlebar-left">
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
      <button class="tbtn" type="button" aria-label="菜单" @click="emit('menu')">
        <Menu :size="16" :stroke-width="1.75"/>
      </button>
    </div>
    <div class="titlebar-middle">
      <input
          ref="fieldEl"
          v-model="address"
          class="field"
          type="text"
          spellcheck="false"
          placeholder="请输入地址"
          :title="address"
          @focus="onFocus"
          @blur="onBlur"
          @keydown="onKeydown"
      />
    </div>
    <div class="titlebar-right">
      <button class="wbtn" type="button" aria-label="最小化" @click="appWindow?.minimize()">
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
  </div>
</template>

<style scoped>
/* ============ 标题栏基础布局 ============ */
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
  box-sizing: border-box;
}

/* 左右两侧固定，中间自适应 */
.titlebar-left,
.titlebar-right {
  display: flex;
  align-items: stretch;
  height: 100%;
  flex-shrink: 0;
}

.titlebar-middle {
  flex: 1 1 auto;
  min-width: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 0 12px;
}

/* ============ 左侧：Logo / 分隔线 / 导航按钮 ============ */
.titlebar-left {
  gap: 4px;
  padding-left: 8px;
}

.logo {
  width: 38px; /* 20 / (67/128) ≈ 38 */
  height: 38px;
  object-fit: contain;
  align-self: center;
  pointer-events: none;
  margin: -9px 0; /* 抵消多出来的上下留白，避免撑高标题栏 */
}

.divider {
  width: 1px;
  height: 16px;
  align-self: center;
  margin: 0 6px;
  background-color: var(--divider);
  flex-shrink: 0;
}

/* ============ 通用按钮 ============ */
.tbtn,
.wbtn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border: none;
  background-color: transparent;
  color: var(--text-dim);
  cursor: default;
  padding: 0;
  height: 100%;
  border-radius: 0;
  position: relative;
  transition: background-color 0.18s ease,
  color 0.18s ease,
  transform 0.08s ease;
  -webkit-app-region: no-drag;
  will-change: background-color, color;
}

/* 图标本身也跟随过渡 */
.tbtn :deep(svg),
.wbtn :deep(svg) {
  transition: color 0.18s ease, transform 0.18s ease;
}

.tbtn:focus-visible,
.wbtn:focus-visible {
  outline: 2px solid var(--border);
  outline-offset: -2px;
}

/* ============ 左侧工具按钮 ============ */
.tbtn {
  width: 34px;
}

.tbtn:hover {
  background-color: var(--hover);
  color: var(--text);
}

.tbtn:hover :deep(svg) {
  transform: scale(1.08);
}

.tbtn:active {
  background-color: var(--press);
  transform: scale(0.96);
}

.tbtn:disabled {
  opacity: 0.35;
  cursor: default;
  background-color: transparent;
  transform: none;
}

/* ============ 中间：地址 / 搜索输入框 ============ */
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
  transition: background-color 120ms ease,
  border-color 120ms ease;
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

/* ============ 右侧：窗口控制按钮 ============ */
.titlebar-right {
  gap: 0;
}

.wbtn {
  width: 46px;
}

.wbtn:hover {
  background-color: var(--hover);
  color: var(--text);
}

.wbtn:hover :deep(svg) {
  transform: scale(1.08);
}

.wbtn:active {
  background-color: var(--press);
  transform: scale(0.96);
}

/* 关闭按钮：悬浮变红，红色也带过渡 */
.wbtn--close:hover {
  background-color: var(--accent);
  color: #fff;
}

.wbtn--close:active {
  background-color: var(--accent-soft);
  color: #fff;
}

/* ============ 减小动画偏好 ============ */
@media (prefers-reduced-motion: reduce) {
  .tbtn,
  .wbtn,
  .tbtn :deep(svg),
  .wbtn :deep(svg),
  .field {
    transition: none;
    transform: none;
  }
}
</style>