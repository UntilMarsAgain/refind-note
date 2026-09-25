<script setup lang="ts">
import {
  computed,
  onMounted,
  onUnmounted,
  ref,
  watch,
  type Component,
} from "vue";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  Copy,
  Menu,
  Minus,
  Monitor,
  Moon,
  ArrowLeft,
  ArrowRight,
  Square,
  Sun,
  X,
} from "@lucide/vue";
import {
  cycleThemeMode,
  themeMode,
  type ThemeMode,
} from "../theme";

/**
 * 自绘标题栏。
 *
 * 中间是一个「地址栏式」输入框：静止时显示当前笔记的标题（由 title 传入），
 * 聚焦后可编辑。依赖 tauri.conf.json 的 `decorations: false`，以及
 * capabilities 里的 core:window:allow-{minimize,toggle-maximize,close,start-dragging}。
 */

const props = defineProps<{
  title: string;
  canBack?: boolean;
  canForward?: boolean;
}>();

const emit = defineEmits<{
  (e: "back"): void;
  (e: "forward"): void;
  (e: "menu"): void;
  (e: "submit", value: string): void;
}>();

const appWindow = getCurrentWindow();
const isMaximized = ref(false);

const fieldEl = ref<HTMLInputElement | null>(null);
/** 静止时显示笔记标题，聚焦后可编辑 */
const draft = ref("");
/** 最近一次提交的值，Esc 或失焦时回退到它 */
let committed = "";

// 标题由外部（当前笔记）驱动：变化时同步过来，
// 但用户正在输入时不要抢走他编辑中的内容。
watch(
  () => props.title,
  (title) => {
    committed = title;
    // 只改值，**绝不碰焦点**：地址栏同步不该把光标从输入框里赶走。
    // prop 只在「应用完成了一次导航」时变化，所以正在输入时不会被抢走内容。
    if (draft.value !== title) {
      draft.value = title;
    }
  },
  { immediate: true },
);

/** 循环切换按钮显示当前模式，点一下换到下一个 */
const themeIcons: Record<ThemeMode, Component> = {
  system: Monitor,
  light: Sun,
  dark: Moon,
};

const themeIcon = computed(() => themeIcons[themeMode.value]);

let unlistenResized: (() => void) | undefined;

onMounted(async () => {
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
    // 刻意**不 blur**：提交后光标留在地址栏，方便接着敲下一条；
    // 回显（规范地址）由 props.title 的 watcher 负责，它只改值、不动焦点。
  } else if (event.key === "Escape") {
    draft.value = committed;
    fieldEl.value?.blur();
  }
}

function onBlur() {
  // 没提交就退回上一次的值，行为对齐浏览器地址栏
  // 失焦一律以外部（解析结果）为准回显 —— **无论刚才有没有发生跳转**，
  // 否则输入框会停在用户敲的原文上，与地址栏该显示的内容不一致。
  // （解析失败时 props.title 本身就是用户写的原文，所以那条例外仍然成立。）
  committed = props.title;
  draft.value = props.title;
}
</script>

<template>
  <!-- "deep" = 子树内任意位置都能拖动。
       输入框与按钮属于 Tauri 的 CLICKABLE_TAGS，会自动阻断拖动，所以不必逐个排除。 -->
  <header class="titlebar" data-tauri-drag-region="deep">
    <div class="titlebar__start">
      <!--
        应用图标：用 currentColor 填色、靠 mask 抠出书形，
        所以浅色/深色主题只要给它不同的 color 就自动适配（见 .logo 的 color）。
        内联而不是 <img> —— 只有内联的 SVG 才吃得到 currentColor。
      -->
      <svg
        class="logo"
        xmlns="http://www.w3.org/2000/svg"
        width="128"
        height="128"
        viewBox="0 0 128 128"
        aria-hidden="true"
        focusable="false"
      >
        <defs>
          <mask id="refindKnock">
            <rect x="8" y="8" width="112" height="112" rx="26" fill="#fff" />
            <path
              d="M76 93H52A17 17 0 0 1 35 76V52A17 17 0 0 1 52 35h24A17 17 0 0 1 93 52v24"
              fill="none"
              stroke="#000"
              stroke-width="9"
              stroke-linecap="round"
              stroke-linejoin="round"
            />
            <path
              d="M56.5 47h15a4.5 4.5 0 0 1 4.5 4.5v29L64 72.8 52 80.5v-29A4.5 4.5 0 0 1 56.5 47Z"
              fill="#000"
              stroke="#000"
              stroke-width="2.5"
              stroke-linejoin="round"
            />
          </mask>
        </defs>
        <rect
          x="8"
          y="8"
          width="112"
          height="112"
          rx="26"
          fill="currentColor"
          mask="url(#refindKnock)"
        />
      </svg>
      <span class="divider" />

      <button
        class="tbtn"
        type="button"
        aria-label="后退"
        :disabled="!canBack"
        @click="emit('back')"
      >
        <ArrowLeft :size="16" :stroke-width="1.75" />
      </button>

      <button
        class="tbtn"
        type="button"
        aria-label="前进"
        :disabled="!canForward"
        @click="emit('forward')"
      >
        <ArrowRight :size="16" :stroke-width="1.75" />
      </button>

      <!-- 菜单按钮留作占位：展开内容之后再接 -->
      <button
        class="tbtn"
        type="button"
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

    <button
      class="tbtn tbtn--theme"
      type="button"
      aria-label="切换外观"
      @click="cycleThemeMode()"
    >
      <component :is="themeIcon" :size="16" :stroke-width="1.75" />
    </button>

    <div class="titlebar__controls">
      <button
        class="wbtn"
        type="button"
        aria-label="最小化"
        @click="appWindow.minimize()"
      >
        <Minus :size="14" />
      </button>
      <button
        class="wbtn"
        type="button"
        :aria-label="isMaximized ? '向下还原' : '最大化'"
        @click="appWindow.toggleMaximize()"
      >
        <Copy v-if="isMaximized" :size="12" />
        <Square v-else :size="12" />
      </button>
      <button
        class="wbtn wbtn--close"
        type="button"
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
/*
 * 图标是一块实心圆角方块 + 抠空的「敲门/书」形，所以它的颜色就是**方块的颜色**：
 * 深色主题下要是深色方块、浅色主题下要是浅色方块（与这张图原样渲染出来一致）。
 * `--surface` 正好是这个走向：深色下 #372728、浅色下 #ffffff。
 * （用 --text 会反过来 —— 深色下变成浅色方块，那就不是这张图的样子了。）
 */
.logo {
  color: var(--surface);
}
</style>
