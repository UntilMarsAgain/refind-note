<script setup lang="ts">
import { onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import FloatingTools from "./components/FloatingTools.vue";
import TitleBar from "./components/TitleBar.vue";
import NoteContent from "./components/NoteContent.vue";
import PageHeader from "./components/PageHeader.vue";
import WindowResizeHandles from "./components/WindowResizeHandles.vue";

/** 与 Rust 端 `Note` 结构对应 */
interface Note {
  title: string;
  html: string;
}

const WIDTH_PREFERENCE_KEY = "refind-note:limit-width";

/** 默认限宽；只有明确存过「0」才不限宽 */
function readLimitWidth(): boolean {
  try {
    return localStorage.getItem(WIDTH_PREFERENCE_KEY) !== "0";
  } catch {
    // 存储不可用时按默认值
    return true;
  }
}

const note = ref<Note | null>(null);
const loadError = ref("");
/** 正文滚下去之后，页头收起并贴顶冻结 */
const scrolled = ref(false);
/** 滚动容器，滚动到底/回顶都作用在它上面 */
const scrollEl = ref<HTMLElement | null>(null);
/** 正文是否限制为阅读栏宽度 */
const limitWidth = ref(readLimitWidth());

onMounted(async () => {
  try {
    note.value = await invoke<Note>("load_note");
  } catch (error) {
    loadError.value = String(error);
  }
});

/**
 * 收起页头。
 *
 * 收起会让页头矮约 20px，若笔记恰好只比视口高一点点，就会出现
 * 「收起 -> 内容变矮 -> scrollTop 被夹回 -> 展开」的临界抖动。
 * 所以展开比收起更早触发，留出回差。
 */
function onScroll(event: Event) {
  const el = event.currentTarget;
  if (!(el instanceof HTMLElement)) {
    return;
  }

  const { scrollTop } = el;
  if (!scrolled.value && scrollTop > 32) {
    scrolled.value = true;
  } else if (scrolled.value && scrollTop < 12) {
    scrolled.value = false;
  }
}

function toggleWidth() {
  limitWidth.value = !limitWidth.value;
  try {
    localStorage.setItem(WIDTH_PREFERENCE_KEY, limitWidth.value ? "1" : "0");
  } catch {
    // 存不下不影响本次会话
  }
}

function scrollToTop() {
  scrollEl.value?.scrollTo({ top: 0, behavior: "smooth" });
}

function scrollToBottom() {
  const el = scrollEl.value;
  if (el) {
    el.scrollTo({ top: el.scrollHeight, behavior: "smooth" });
  }
}

// 各处入口目前都是占位，先在代码里留痕，
// 避免以后看代码时误以为「点了没反应」是 bug。
function onSearch() {
  // TODO: 打开搜索面板
}

function onMenu() {
  // TODO: 展开菜单（展开内容之后再接）
}

function onSubmit(value: string) {
  // TODO: 按标题打开对应的笔记
  console.debug("open note:", value);
}

function onAction(name: string) {
  // TODO: 接入各页面操作
  console.debug("page action:", name);
}
</script>

<template>
  <WindowResizeHandles />

  <div class="app">
    <TitleBar
      :title="note?.title ?? ''"
      @search="onSearch"
      @menu="onMenu"
      @submit="onSubmit"
    />

    <main
      ref="scrollEl"
      class="app__body"
      :class="{ 'app__body--wide': !limitWidth }"
      @scroll.passive="onScroll"
    >
      <div class="app__column" :class="{ 'app__column--wide': !limitWidth }">
        <template v-if="note">
          <PageHeader
            :title="note.title"
            :collapsed="scrolled"
            @action="onAction"
          />
          <NoteContent :html="note.html" />
        </template>

        <p v-else-if="loadError" class="app__error">{{ loadError }}</p>
      </div>
    </main>
  </div>

  <FloatingTools
    :limited="limitWidth"
    @toggle-width="toggleWidth"
    @scroll-top="scrollToTop"
    @scroll-bottom="scrollToBottom"
  />
</template>

<style scoped>
.app {
  display: flex;
  flex-direction: column;
  height: 100vh;
}

.app__body {
  flex: 1 1 auto;
  overflow: auto;
  /* 顶部留白交给 PageHeader，这样它贴顶冻结时不会有缝 */
  padding: 0 32px 64px;
  transition: padding 200ms ease;
}

/* 取消限宽时给右下角那组悬浮按钮让出位置，
   否则正文会跑到它们底下被挡住 */
.app__body--wide {
  padding-right: 64px;
}

/* 阅读栏：页头与正文共用同一条，限宽并居中 */
.app__column {
  max-width: var(--reading-width);
  margin-inline: auto;
  /* 切换限宽时让版心平滑变化，而不是硬跳 */
  transition: max-width 200ms ease;
}

/* 取消限宽：两侧只留容器自己的少量内边距。
   用 100% 而不是 none —— none 不可插值，上面的过渡会直接断掉。 */
.app__column--wide {
  max-width: 100%;
}

@media (prefers-reduced-motion: reduce) {
  .app__body,
  .app__column {
    transition: none;
  }
}

.app__error {
  margin: 28px 0 0;
  color: var(--accent-soft);
  font-size: 14px;
}
</style>
