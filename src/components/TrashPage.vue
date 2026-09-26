<script setup lang="ts">
import type { TrashEntry } from "../api-types";
/**
 * 回收站（`special:trash`）。
 *
 * 列出删过的笔记，每条可以**还原**或**立即清除**。
 * 超过保留期的条目由启动时的自动维护清掉（天数在设置里改），这一页只显示策略与上次清理时间。
 *
 * 三点刻意如此：
 * - **不做自动清理之外的后台删除**：自动那部分有明确的策略与可查的执行时间；
 * - **立即清除不顺手回收内容块**：那件事交给「数据库回收」页（下面链过去），由用户决定；
 * - 如实说明能做什么、不能做什么。
 */
import { onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";


const props = defineProps<{
  /** 保留天数（来自设置，用于说明策略与标记"可清理"） */
  keepDays: number;
  /** 上次自动清理的时间（空 = 从未） */
  lastPurge: string;
}>();

const emit = defineEmits<{
  (e: "open", address: string): void;
}>();

const entries = ref<TrashEntry[]>([]);
const loading = ref(true);
const error = ref("");
/** 正在处理哪一条（阻止连点，也让用户看到在处理谁） */
const busyTitle = ref("");

/**
 * 是否已到可清理的年纪。
 *
 * 删除时间读不出来的条目不参与 —— 后端也不会清它们，两边的判断必须一致，
 * 否则会出现"标记了可清理、自动清理却没动它"。
 */
function isExpired(item: TrashEntry): boolean {
  return item.days_old !== null && item.days_old >= props.keepDays;
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) {
    return `${bytes} 字节`;
  }
  if (bytes < 1024 * 1024) {
    return `${(bytes / 1024).toFixed(1)} KiB`;
  }
  return `${(bytes / 1024 / 1024).toFixed(1)} MiB`;
}

function formatTime(value: string): string {
  if (!value) {
    return "从未";
  }
  const at = new Date(value);
  return Number.isNaN(at.getTime()) ? value : at.toLocaleString();
}

async function refresh() {
  loading.value = true;
  try {
    entries.value = await invoke<TrashEntry[]>("list_trash");
    error.value = "";
  } catch (reason) {
    error.value = String(reason);
  } finally {
    loading.value = false;
  }
}

async function run(title: string, action: () => Promise<unknown>) {
  busyTitle.value = title;
  error.value = "";
  try {
    await action();
    await refresh();
  } catch (reason) {
    error.value = String(reason);
  } finally {
    busyTitle.value = "";
  }
}

function restore(item: TrashEntry) {
  return run(item.title, () => invoke("restore_note", { title: item.title }));
}

function purgeOne(item: TrashEntry) {
  return run(item.title, () => invoke("purge_trash_entry", { title: item.title }));
}

onMounted(refresh);
</script>

<template>
  <section class="trash">
    <h1 class="trash__title">回收站</h1>
    <p class="trash__lead">
      删过的笔记都在这里，<strong>历史一条没丢</strong>，可以随时还原。
      超过 {{ keepDays }} 天的条目会在启动时自动清理（天数可在设置里改）。
    </p>

    <p v-if="error" class="trash__error">{{ error }}</p>
    <p v-else-if="loading" class="trash__hint">正在读取…</p>

    <template v-else>
      <p v-if="entries.length === 0" class="trash__hint">回收站是空的。</p>

      <ul v-else class="trash__list">
        <li v-for="item in entries" :key="item.title" class="trash__item">
          <div class="trash__info">
            <span class="trash__name">{{ item.title }}</span>
            <span class="trash__meta">
              {{ formatTime(item.deleted_at) }}
              <template v-if="item.days_old !== null"> · 已 {{ item.days_old }} 天</template>
              · {{ formatBytes(item.bytes) }}
            </span>
          </div>

          <span v-if="isExpired(item)" class="trash__badge">可清理</span>
          <span v-else-if="item.days_old === null" class="trash__badge trash__badge--dim">
            时间未知，不自动清
          </span>

          <div class="trash__item-actions">
            <button
              class="trash__btn"
              type="button"
              :disabled="busyTitle === item.title"
              @click="restore(item)"
            >
              还原
            </button>
            <button
              class="trash__btn trash__btn--danger"
              type="button"
              :disabled="busyTitle === item.title"
              @click="purgeOne(item)"
            >
              立即清除
            </button>
          </div>
        </li>
      </ul>

      <p class="trash__footer">
        上次自动清理：{{ formatTime(lastPurge) }}
        <span class="trash__sep">|</span>
        立即清除只删这一条，空间要在这里回收：
        <button class="trash__link" type="button" @click="emit('open', 'special:gc')">
          去数据库回收 →
        </button>
      </p>
    </template>
  </section>
</template>

<style scoped>
.trash {
  /* 宽度交给 App 的阅读栏（`.app__column`）：限宽时 1080，不限宽时铺满。
     这里自设 max-width 会把它盖住，页面对"限宽"按钮就没反应了。 */
  margin: 0 auto;
  padding: 28px 20px 64px;
}

.trash__title {
  margin: 0 0 6px;
  font-size: 22px;
}

.trash__lead {
  margin: 0 0 20px;
  color: var(--text-dim);
  font-size: 13px;
  line-height: 1.7;
}

.trash__list {
  margin: 0;
  padding: 0;
  list-style: none;
}

.trash__item {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 10px;
  padding: 10px 0;
  border-bottom: 1px solid var(--border);
}

.trash__info {
  display: flex;
  flex-direction: column;
  gap: 2px;
  min-width: 0;
  flex: 1 1 auto;
}

.trash__name {
  font-size: 14px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.trash__meta {
  color: var(--text-dim);
  font-size: 12px;
}

.trash__badge {
  flex-shrink: 0;
  padding: 0 6px;
  border-radius: 9px;
  background: var(--code-bg);
  color: var(--link-missing);
  font-size: 11px;
}

.trash__badge--dim {
  color: var(--text-dim);
}

.trash__item-actions {
  display: flex;
  flex-shrink: 0;
  gap: 6px;
}

.trash__btn {
  padding: 4px 10px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background-color: transparent;
  color: var(--text);
  font-size: 12px;
  cursor: pointer;
}

.trash__btn:hover:not(:disabled) {
  background-color: var(--hover);
}

.trash__btn--danger {
  color: var(--link-missing);
}

.trash__btn:disabled {
  opacity: 0.45;
  cursor: default;
}

.trash__footer {
  margin: 18px 0 0;
  color: var(--text-dim);
  font-size: 12px;
  line-height: 1.9;
}

.trash__sep {
  margin: 0 8px;
}

.trash__link {
  padding: 0;
  border: 0;
  background-color: transparent;
  color: var(--accent);
  font-size: 12px;
  cursor: pointer;
}

.trash__link:hover {
  text-decoration: underline;
}

.trash__hint {
  color: var(--text-dim);
  font-size: 13px;
}

.trash__error {
  color: var(--link-missing);
  font-size: 13px;
}
</style>
