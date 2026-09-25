<script setup lang="ts">
import { computed, onMounted, onUnmounted, ref, type Component } from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  Check,
  Copy,
  Menu,
  Minus,
  Monitor,
  Moon,
  Search,
  Square,
  Sun,
  X,
} from "@lucide/vue";
import {
  cycleThemeMode,
  nextThemeMode,
  setThemeMode,
  themeMode,
  themeOptions,
  type ThemeMode,
} from "../theme";

/**
 * 自绘标题栏。
 *
 * 中间是一个「地址栏式」输入框：静止时显示窗口标题，聚焦后可编辑。
 * 依赖 tauri.conf.json 的 `decorations: false`，以及 capabilities 里的
 * core:window:allow-{minimize,toggle-maximize,close,title,start-dragging}。
 */

const emit = defineEmits<{
  (e: "search"): void;
  (e: "submit", value: string): void;
}>();

const appWindow = getCurrentWindow();
const isMaximized = ref(false);

const fieldEl = ref<HTMLInputElement | null>(null);
/** 静止时是窗口标题，聚焦后可编辑 */
const draft = ref("");
/** 最近一次提交的值，Esc 或失焦时回退到它 */
let committed = "";

const menuEl = ref<HTMLElement | null>(null);
const menuOpen = ref(false);

/** 循环切换按钮显示当前模式，点一下按 themeOptions 的顺序换到下一个 */
const themeIcons: Record<ThemeMode, Component> = {
  system: Monitor,
  light: Sun,
  dark: Moon,
};

const themeIcon = computed(() => themeIcons[themeMode.value]);

const themeTip = computed(() => {
  const current = themeOptions.find((item) => item.value === themeMode.value);
  const next = themeOptions.find((item) => item.value === nextThemeMode.value);
  return `外观：${current?.label ?? ""}（点击切到 ${next?.label ?? ""}）`;
});

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

  document.addEventListener("pointerdown", onPointerDown);
});

onUnmounted(() => {
  unlistenResized?.();
  document.removeEventListener("pointerdown", onPointerDown);
});

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

/** 点菜单面板以外的地方就收起它 */
function onPointerDown(event: PointerEvent) {
  if (!menuOpen.value) {
    return;
  }
  const target = event.target;
  if (target instanceof Node && !menuEl.value?.contains(target)) {
    menuOpen.value = false;
  }
}

function chooseTheme(next: ThemeMode) {
  setThemeMode(next);
  menuOpen.value = false;
}
</script>

<template>
  <!-- "deep" = 子树内任意位置都能拖动。
       输入框与按钮属于 Tauri 的 CLICKABLE_TAGS，会自动阻断拖动，所以不必逐个排除。 -->
  <header class="titlebar" data-tauri-drag-region="deep">
    <div class="titlebar__start">
      <!-- 先放占位图标，之后再换成真正的应用图标 -->
      <img class="logo" src="/tauri.svg" alt="" draggable="false" />
      <span class="divider" />

      <button
        class="tbtn"
        type="button"
        title="搜索"
        aria-label="搜索"
        @click="emit('search')"
      >
        <Search :size="16" :stroke-width="1.75" />
      </button>

      <div ref="menuEl" class="menu">
        <button
          class="tbtn"
          type="button"
          title="菜单"
          aria-label="菜单"
          aria-haspopup="menu"
          :aria-expanded="menuOpen"
          @click="menuOpen = !menuOpen"
        >
          <Menu :size="16" :stroke-width="1.75" />
        </button>

        <!-- "false" = 面板内不留拖动区，否则点标题或空白会把窗口拖走 -->
        <div
          v-if="menuOpen"
          class="menu__panel"
          role="menu"
          data-tauri-drag-region="false"
        >
          <p class="menu__title">外观</p>
          <button
            v-for="option in themeOptions"
            :key="option.value"
            class="menu__item"
            type="button"
            role="menuitemradio"
            :aria-checked="themeMode === option.value"
            @click="chooseTheme(option.value)"
          >
            <Check
              v-if="themeMode === option.value"
              class="menu__check"
              :size="14"
            />
            <span v-else class="menu__check" />
            {{ option.label }}
          </button>
        </div>
      </div>
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

    <button
      class="tbtn tbtn--theme"
      type="button"
      :title="themeTip"
      aria-label="切换外观"
      @click="cycleThemeMode()"
    >
      <component :is="themeIcon" :size="16" :stroke-width="1.75" />
    </button>

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
  /* 与正文同一个变量，两者之间不留分界 */
  background: var(--bg);
  transition: background-color 160ms ease;
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

/* 循环切换按钮紧挨着窗口按钮，但要留一条缝，
   免得被误认成最小化/最大化那一组 */
.tbtn--theme {
  margin-right: 6px;
}

.menu {
  position: relative;
}

.menu__panel {
  position: absolute;
  top: calc(100% + 4px);
  left: 0;
  z-index: 20;
  min-width: 136px;
  padding: 6px;
  border: 1px solid var(--border);
  border-radius: 8px;
  background: var(--surface);
  box-shadow: 0 8px 24px rgba(0, 0, 0, 0.35);
}

.menu__title {
  margin: 2px 8px 6px;
  color: var(--text-dim);
  font-size: 12px;
}

.menu__item {
  display: flex;
  align-items: center;
  gap: 6px;
  width: 100%;
  padding: 6px 8px;
  border: 0;
  border-radius: 6px;
  background: transparent;
  color: var(--text);
  font-size: 13.5px;
  text-align: left;
  cursor: default;
}

.menu__item:hover {
  background: var(--hover);
}

/* 选中态与占位用同一个宽度，避免三行的文字左右错位 */
.menu__check {
  display: inline-flex;
  flex: 0 0 auto;
  width: 14px;
  color: var(--accent-soft);
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
