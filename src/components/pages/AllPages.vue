<script setup lang="ts">
import { onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import type { NoteSummary } from "../../bindings/note.ts";

/**
 * 全部页面：列出仓库里现有的笔记，点一条去那一页。
 */
const emit = defineEmits<{
    (e: "navigate", title: string): void;
}>();

const notes = ref<NoteSummary[]>([]);
const error = ref("");

onMounted(async () => {
    try {
        notes.value = await invoke<NoteSummary[]>("list_notes");
    } catch (reason) {
        error.value = String(reason);
    }
});
</script>

<template>
  <div class="all">
    <header class="all__head">
      <h1 class="all__title">全部页面</h1>
      <p class="all__count">共 {{ notes.length }} 篇</p>
    </header>

    <p v-if="error" class="all__error">{{ error }}</p>
    <p v-else-if="notes.length === 0" class="all__hint">
      仓库中暂无笔记，可在地址栏输入名称创建。
    </p>
    <ol v-else class="all__list">
      <li v-for="note in notes" :key="note.key">
        <button type="button" class="all__item" @click="emit('navigate', note.title)">
          <span class="all__name">{{ note.title }}</span>
          <span class="all__meta">第 {{ note.rev }} 版 · {{ note.bytes }} 字节 · 修改于 {{ note.modified }}</span>
        </button>
      </li>
    </ol>
  </div>
</template>

<style scoped>
.all__head {
  padding: 28px 0 8px;
}

.all__title {
  margin: 0;
  font-size: 26px;
  font-weight: 600;
  line-height: 1.35;
}

.all__count {
  margin: 6px 0 0;
  color: var(--text-dim);
  font-size: 13px;
}

.all__list {
  margin: 14px 0 0;
  padding: 0;
  list-style: none;
  border-top: 1px solid var(--border);
}

.all__item {
  display: block;
  width: 100%;
  padding: 10px 8px;
  border: 0;
  border-bottom: 1px solid var(--border);
  background: transparent;
  color: var(--text);
  font: inherit;
  text-align: left;
  cursor: pointer;
}

.all__item:hover {
  background: var(--hover);
}

.all__name {
  display: block;
  font-size: 15px;
}

.all__meta {
  display: block;
  margin-top: 2px;
  color: var(--text-dim);
  font-size: 12.5px;
}

.all__hint,
.all__error {
  margin: 28px 0 0;
  color: var(--text-dim);
  font-size: 13.5px;
}

.all__error {
  color: var(--danger);
}
</style>
