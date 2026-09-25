<script setup lang="ts">
import { onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import TitleBar from "./components/TitleBar.vue";
import NoteContent from "./components/NoteContent.vue";
import WindowResizeHandles from "./components/WindowResizeHandles.vue";

/** 与 Rust 端 `Note` 结构对应 */
interface Note {
  title: string;
  html: string;
}

const note = ref<Note | null>(null);
const loadError = ref("");

onMounted(async () => {
  try {
    note.value = await invoke<Note>("load_note");
  } catch (error) {
    loadError.value = String(error);
  }
});

// 标题栏上的两个入口目前是占位，先在代码里留痕，
// 避免以后看代码时误以为「点了没反应」是 bug。
function onSearch() {
  // TODO: 打开搜索面板
}

function onSubmit(value: string) {
  // TODO: 按标题打开对应的笔记
  console.debug("open note:", value);
}
</script>

<template>
  <WindowResizeHandles />

  <div class="app">
    <TitleBar
      :title="note?.title ?? ''"
      @search="onSearch"
      @submit="onSubmit"
    />

    <main class="app__body">
      <NoteContent v-if="note" :html="note.html" />
      <p v-else-if="loadError" class="app__error">{{ loadError }}</p>
    </main>
  </div>
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
  padding: 28px 32px 64px;
}

.app__error {
  max-width: var(--reading-width);
  margin-inline: auto;
  margin-block: 0;
  color: var(--accent-soft);
  font-size: 14px;
}
</style>
