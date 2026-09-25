<script setup lang="ts">
import { computed, nextTick, ref, watch } from "vue";
import { Search } from "@lucide/vue";

/**
 * 笔记搜索：按标题子串匹配。
 *
 * 刻意**不往后端跑**——标签栏已经把整份笔记列表拿在手里了，本地过滤更快也更简单。
 * 等笔记多到一次拿不完（要分页）时，再挪到后端做。
 */

/** 与 Rust 端 `NoteSummary` 对应（这里只用到 title） */
interface NoteSummary {
  key: string;
  title: string;
  rev: number;
  modified: string;
}

const props = defineProps<{
  notes: NoteSummary[];
  open: boolean;
}>();

const emit = defineEmits<{
  (e: "close"): void;
  (e: "open-note", title: string): void;
}>();

const keyword = ref("");
const inputEl = ref<HTMLInputElement | null>(null);

/** 一次最多列这么多，免得列表长到没法用 */
const MAX_RESULTS = 40;

const results = computed(() => {
  const needle = keyword.value.trim().toLowerCase();
  if (!needle) {
    return props.notes.slice(0, MAX_RESULTS);
  }
  return props.notes
    .filter((item) => item.title.toLowerCase().includes(needle))
    .slice(0, MAX_RESULTS);
});

watch(
  () => props.open,
  async (isOpen) => {
    if (!isOpen) {
      return;
    }
    keyword.value = "";
    await nextTick();
    inputEl.value?.focus();
  },
);

function choose(title: string) {
  emit("open-note", title);
  emit("close");
}
</script>

<template>
  <div v-if="open" class="search" @click.self="emit('close')">
    <div class="search__panel">
      <label class="search__field">
        <Search :size="15" :stroke-width="1.9" />
        <input
          ref="inputEl"
          v-model="keyword"
          type="text"
          placeholder="按标题搜索笔记"
          @keydown.esc="emit('close')"
          @keydown.enter="results[0] && choose(results[0].title)"
        />
      </label>

      <ul class="search__list">
        <li v-for="item in results" :key="item.key">
          <button type="button" @click="choose(item.title)">
            {{ item.title }}
          </button>
        </li>
        <li v-if="results.length === 0" class="search__empty">没有匹配的笔记</li>
      </ul>
    </div>
  </div>
</template>

<style scoped>
.search {
  position: fixed;
  inset: 0;
  /* 高于 sticky 页头（10）与悬浮按钮（30），低于窗口缩放热区（50） */
  z-index: 40;
  display: flex;
  justify-content: center;
  align-items: flex-start;
  padding: calc(var(--titlebar-height) + 12px) 16px 16px;
  background: rgba(0, 0, 0, 0.35);
}

.search__panel {
  width: min(520px, 100%);
  padding: 10px;
  border: 1px solid var(--border);
  border-radius: 10px;
  background: var(--bg);
  box-shadow: 0 12px 32px rgba(0, 0, 0, 0.4);
}

.search__field {
  display: flex;
  align-items: center;
  gap: 8px;
  height: 32px;
  padding: 0 10px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: var(--field-bg);
  color: var(--text-dim);
}

.search__field input {
  flex: 1 1 auto;
  min-width: 0;
  height: 100%;
  border: 0;
  background: transparent;
  color: var(--text);
  font: inherit;
  font-size: 13.5px;
  outline: none;
}

.search__list {
  max-height: 320px;
  margin: 8px 0 0;
  padding: 0;
  overflow-y: auto;
  list-style: none;
}

.search__list button {
  appearance: none;
  display: block;
  width: 100%;
  padding: 7px 10px;
  border: 0;
  border-radius: 6px;
  background: transparent;
  color: var(--text-dim);
  font-size: 13.5px;
  line-height: 1.5;
  text-align: left;
  cursor: pointer;
}

.search__list button:hover {
  background: var(--hover);
  color: var(--text);
}

.search__empty {
  padding: 8px 10px;
  color: var(--text-dim);
  font-size: 13px;
}
</style>
