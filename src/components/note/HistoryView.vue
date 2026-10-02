<!--
  Refind Note is a note-taking software.
  Copyright (C) 2026 Until Mars Again

  This program is free software: you can redistribute it and/or modify
  it under the terms of the GNU Affero General Public License as published by
  the Free Software Foundation, either version 3 of the License, or
  (at your option) any later version.

  This program is distributed in the hope that it will be useful,
  but WITHOUT ANY WARRANTY; without even the implied warranty of
  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
  GNU Affero General Public License for more details.

  You should have received a copy of the GNU Affero General Public License
  along with this program.  If not, see <http://www.gnu.org/licenses/>.
-->

<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import type { RevisionSummary } from "../../ipc/note.ts";
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
  /** 把某一版重新提上去（回退页） */
  (e: "rollback", rev: number): void;
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
        <div class="history__row">
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

          <!-- 回退到最新版没有意义，所以那一行不给这个出口。
               这一条路**不用先读得懂这一版**：回退页自己会解锁（或整份复制） -->
          <button
            v-if="revision.rev !== latestRev"
            type="button"
            class="history__rollback"
            title="把这一版的内容当成新的一版写上去"
            @click="emit('rollback', revision.rev)"
          >
            回退
          </button>
        </div>

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

.history__row {
  display: flex;
  align-items: center;
  gap: 6px;
}

.history__open {
  display: flex;
  flex: 1 1 auto;
  min-width: 0;
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

/* 「回退」贴着行尾：它与"打开这一版"是两件事，做成两枚按钮免得点错 */
.history__rollback {
  flex: 0 0 auto;
  margin-right: 8px;
  padding: 4px 10px;
  border: 1px solid var(--border);
  border-radius: 999px;
  background: transparent;
  color: var(--text-dim);
  font: inherit;
  font-size: 12px;
  white-space: nowrap;
  cursor: pointer;
}

.history__rollback:hover {
  border-color: var(--accent-soft);
  background: var(--accent-tint);
  color: var(--text);
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
