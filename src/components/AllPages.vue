<script setup lang="ts">
/**
 * 全部页面（`special:all`）：列出数据库里所有笔记，以及所有可用的特殊页面。
 *
 * 「有哪些特殊页面」**以后端为准**（`special_pages` 命令），前端只决定怎么显示它们；
 * 否则每加一个特殊页面都要在前端再登记一次，那就成了两个真相。
 */
import { onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";

interface NoteSummary {
  key: string;
  title: string;
}

const emit = defineEmits<{
  (e: "open", address: string): void;
  (e: "open-new", address: string): void;
}>();

/** 特殊页面的显示名；没登记的退回 `special:<页面名>` */
const PAGE_LABELS: Record<string, string> = {
  all: "全部页面 · 正在这里",
  newtab: "新标签页",
  settings: "设置",
};

function labelOf(page: string): string {
  return PAGE_LABELS[page] ?? `special:${page}`;
}

const notes = ref<NoteSummary[]>([]);
const pages = ref<string[]>([]);
const error = ref("");
const loading = ref(true);

/** Ctrl/Cmd + 点击＝在新标签页打开，与正文里的内部链接一致 */
function go(address: string, event: MouseEvent) {
  if (event.ctrlKey || event.metaKey) {
    emit("open-new", address);
    return;
  }
  emit("open", address);
}

onMounted(async () => {
  try {
    notes.value = await invoke<NoteSummary[]>("list_notes");
    pages.value = await invoke<string[]>("special_pages");
  } catch (reason) {
    error.value = String(reason);
  } finally {
    loading.value = false;
  }
});
</script>

<template>
  <section class="all">
    <h1 class="all__title">全部页面</h1>

    <p v-if="error" class="all__error">{{ error }}</p>
    <p v-else-if="loading" class="all__hint">正在读取…</p>

    <template v-else>
      <h2 class="all__section">
        笔记 <span class="all__count">{{ notes.length }}</span>
      </h2>
      <p v-if="notes.length === 0" class="all__hint">
        还没有任何笔记。去 <code>special:newtab</code> 建一篇。
      </p>
      <ul v-else class="all__list">
        <li v-for="note in notes" :key="note.key">
          <button
            class="all__link"
            type="button"
            :title="note.key"
            @click="go(note.title, $event)"
          >
            {{ note.title }}
          </button>
        </li>
      </ul>

      <h2 class="all__section">
        特殊页面 <span class="all__count">{{ pages.length }}</span>
      </h2>
      <ul class="all__list">
        <li v-for="page in pages" :key="page">
          <button
            class="all__link all__link--special"
            type="button"
            @click="go(`special:${page}`, $event)"
          >
            {{ labelOf(page) }}
            <span class="all__raw">special:{{ page }}</span>
          </button>
        </li>
      </ul>
    </template>
  </section>
</template>

<style scoped>
.all {
  max-width: 720px;
  margin: 0 auto;
  padding: 28px 20px 64px;
}

.all__title {
  margin: 0 0 18px;
  font-size: 22px;
}

.all__section {
  margin: 22px 0 10px;
  padding-bottom: 6px;
  border-bottom: 1px solid var(--border);
  font-size: 14px;
  font-weight: 500;
  color: var(--text-dim);
}

.all__count {
  margin-left: 6px;
  padding: 1px 7px;
  border-radius: 9px;
  background: var(--code-bg);
  font-size: 12px;
  color: var(--text-dim);
}

.all__list {
  margin: 0;
  padding: 0;
  list-style: none;
  columns: 2;
  column-gap: 18px;
}

.all__list li {
  break-inside: avoid;
}

.all__link {
  display: block;
  width: 100%;
  padding: 5px 8px;
  border: 0;
  border-radius: 6px;
  background: transparent;
  color: var(--accent);
  font-size: 14px;
  text-align: left;
  cursor: pointer;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.all__link:hover {
  background: var(--hover);
}

.all__link--special {
  color: var(--text);
}

.all__raw {
  margin-left: 8px;
  color: var(--text-dim);
  font-size: 12px;
  font-family: var(--mono-font);
}

.all__hint {
  color: var(--text-dim);
  font-size: 13px;
}

.all__hint code {
  padding: 1px 5px;
  border-radius: 4px;
  background: var(--code-bg);
  font-size: 12px;
}

.all__error {
  color: var(--link-missing);
  font-size: 13px;
}
</style>
