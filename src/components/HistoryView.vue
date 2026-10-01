<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import type { RevisionSummary } from "../bindings/note.ts";
import StorageBadge from "./StorageBadge.vue";

/**
 * 一篇笔记的版本历史。
 *
 * **只有提交**：草稿是每篇一个可覆盖槽位，不进事件链，所以它不是这里的一行。
 * 每一行都带上那一版自己的存储状态 —— 同一个文件不同版本可能封装不同。
 */
const props = defineProps<{
  title: string;
}>();

const emit = defineEmits<{
  /** 要去看某一版 */
  (e: "open-version", rev: number): void;
  /** 返回这一篇的阅读页 */
  (e: "cancel"): void;
}>();

const revisions = ref<RevisionSummary[]>([]);
const error = ref("");
const loading = ref(false);

async function load() {
  loading.value = true;
  error.value = "";

  try {
    revisions.value = await invoke<RevisionSummary[]>("list_revisions", {
      title: props.title,
    });
  } catch (reason) {
    revisions.value = [];
    error.value = String(reason);
  } finally {
    loading.value = false;
  }
}

watch(() => props.title, () => void load(), { immediate: true });

/** 时间只显示到分钟：秒对"哪一版"没有帮助，反而把一行挤满 */
function shortTime(at: string): string {
  return at.replace("T", " ").replace(/:\d{2}(\.\d+)?(Z|[+-]\d{2}:\d{2})$/, "");
}

const latestRev = computed(() => revisions.value[0]?.rev ?? 0);
</script>

<template>
  <div class="history">
    <div class="history__head">
      <h1 class="history__title">「{{ title }}」的历史</h1>
      <button type="button" class="history__back" @click="emit('cancel')">
        返回「{{ title }}」
      </button>
    </div>

    <p v-if="loading" class="history__hint">正在读…</p>

    <p v-else-if="error" class="history__error">{{ error }}</p>

    <p v-else-if="revisions.length === 0" class="history__hint">
      这篇还没有提交过。
    </p>

    <ol v-else class="history__list">
      <li v-for="revision in revisions" :key="revision.rev" class="history__item">
        <button
          type="button"
          class="history__open"
          @click="emit('open-version', revision.rev)"
        >
          <span class="history__rev">第 {{ revision.rev }} 版</span>
          <span v-if="revision.rev === latestRev" class="history__latest">最新</span>
          <span class="history__at">{{ shortTime(revision.at) }}</span>
          <span class="history__bytes">{{ revision.bytes }} 字节</span>
          <StorageBadge :protection="revision.protection" :title="title" :reference="String(revision.rev)" />
        </button>

        <p v-if="revision.summary" class="history__summary">{{ revision.summary }}</p>
      </li>
    </ol>
  </div>
</template>

<style scoped>
.history__head {
  display: flex;
  flex-wrap: wrap;
  gap: 12px;
  align-items: baseline;
  justify-content: space-between;
  margin: 28px 0 12px;
}

.history__title {
  margin: 0;
  font-size: 22px;
  font-weight: 600;
  overflow-wrap: anywhere;
}

.history__back {
  padding: 5px 11px;
  border: 1px solid var(--border);
  border-radius: 999px;
  background: transparent;
  color: var(--text-dim);
  font: inherit;
  font-size: 12.5px;
  white-space: nowrap;
  cursor: pointer;
}

.history__back:hover {
  border-color: var(--accent-soft);
  background: var(--accent-tint);
  color: var(--text);
}

.history__list {
  margin: 0;
  padding: 0;
  list-style: none;
}

.history__item {
  border-bottom: 1px solid var(--border);
}

.history__open {
  display: flex;
  flex-wrap: wrap;
  gap: 4px 12px;
  align-items: center;
  width: 100%;
  padding: 10px 8px;
  border: 0;
  border-radius: 6px;
  background: transparent;
  color: var(--text);
  font: inherit;
  font-size: 13.5px;
  text-align: left;
  cursor: pointer;
}

.history__open:hover {
  background: var(--hover);
}

.history__rev {
  font-weight: 600;
}

.history__latest {
  padding: 0 6px;
  border-radius: 999px;
  background: var(--accent-tint);
  color: var(--accent-soft);
  font-size: 11px;
}

.history__at,
.history__bytes {
  color: var(--text-dim);
  font-size: 12.5px;
}

.history__summary {
  margin: 0 0 10px 8px;
  color: var(--text-dim);
  font-size: 12.5px;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}

.history__hint {
  margin: 8px 0;
  color: var(--text-dim);
  font-size: 13.5px;
}

.history__error {
  margin: 8px 0;
  padding: 12px 14px;
  border: 1px solid var(--danger);
  border-left-width: 3px;
  border-radius: 8px;
  color: var(--text);
  font-size: 13.5px;
}
</style>
