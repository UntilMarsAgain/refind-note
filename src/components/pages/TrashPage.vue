<script setup lang="ts">
import { onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { RotateCcw, Trash2 } from "@lucide/vue";
import {
  formatBytes,
  formatTime,
  type TrashEntry,
} from "../../bindings/maintenance.ts";
import { flash } from "../../core/notice.ts";
import { maintenance } from "../../core/preferences.ts";

/**
 * 回收站（`special:trash`）。
 *
 * 删除**没有抹掉任何东西**：日志只是挪到了 `trash/`，历史一条没少，所以这里能还原。
 * 两条出口：还原（挪回去，并补一版"从回收站还原"）、立即清除（不可撤销）。
 *
 * 清除一条**不顺手回收内容块** —— 内容块要等整份日志都不在了才谈得上没人引用，
 * 那是整理页那一轮的事，页脚给一条路过去。
 */
const emit = defineEmits<{
  /** 去别的页面（页脚那条去整理的链接） */
  (e: "navigate", input: string): void;
}>();

const entries = ref<TrashEntry[]>([]);
const loading = ref(false);
const busy = ref(false);
const error = ref("");
/** 正在"确认清除"的那一条（标题）；两步确认，免得点错 */
const confirming = ref("");

async function load() {
  loading.value = true;
  error.value = "";
  try {
    entries.value = await invoke<TrashEntry[]>("list_trash");
  } catch (reason) {
    entries.value = [];
    error.value = String(reason);
  } finally {
    loading.value = false;
  }
}

onMounted(() => void load());

async function restore(entry: TrashEntry) {
  busy.value = true;
  error.value = "";
  try {
    await invoke("restore_note", { title: entry.title });
    flash(`已还原「${entry.title}」`);
    await load();
  } catch (reason) {
    error.value = String(reason);
  } finally {
    busy.value = false;
  }
}

async function purge(entry: TrashEntry) {
  if (confirming.value !== entry.title) {
    confirming.value = entry.title;
    return;
  }
  busy.value = true;
  error.value = "";
  try {
    await invoke("purge_trash_entry", { title: entry.title });
    confirming.value = "";
    flash(`已清除「${entry.title}」，其占用的空间将在仓库整理时释放`);
    await load();
  } catch (reason) {
    error.value = String(reason);
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <section class="trash">
    <div class="trash__head">
      <h1 class="trash__title">回收站</h1>
      <span class="trash__count">{{ entries.length }} 条</span>
    </div>

    <p class="trash__lead">
      删除的笔记会保留在这里，其历史版本完整，可以随时还原。超过
      <strong>{{ maintenance.trash_keep_days }}</strong>
      天的条目将在下次启动时自动清理（可在设置中调整，上次清理：{{ formatTime(maintenance.last_trash_purge) }}）。
    </p>

    <p v-if="error" class="trash__problem">{{ error }}</p>
    <p v-if="loading" class="trash__hint">正在读…</p>
    <p v-else-if="entries.length === 0" class="trash__hint">回收站为空。</p>

    <ol v-else class="trash__list">
      <li v-for="entry in entries" :key="entry.title" class="trash__item">
        <span class="trash__name">{{ entry.title }}</span>
        <span class="trash__meta">
          {{ entry.deleted_at ? formatTime(entry.deleted_at) : "删除时间不详" }}
          ·
          <template v-if="entry.days_old !== null">已 {{ entry.days_old }} 天 · </template>
          {{ formatBytes(entry.bytes) }}
        </span>
        <span v-if="entry.days_old === null" class="trash__badge">时间未知，不自动清理</span>
        <span
            v-else-if="entry.days_old >= maintenance.trash_keep_days"
            class="trash__badge"
        >可清理</span>

        <span class="trash__actions">
          <button type="button" class="trash__btn" :disabled="busy" @click="restore(entry)">
            <RotateCcw :size="13" :stroke-width="1.9"/>
            还原
          </button>
          <button
              type="button"
              class="trash__btn trash__btn--danger"
              :disabled="busy"
              @click="purge(entry)"
          >
            <Trash2 :size="13" :stroke-width="1.9"/>
            {{ confirming === entry.title ? "确认清除" : "立即清除" }}
          </button>
        </span>
      </li>
    </ol>

    <p class="trash__footer">
      「立即清除」只移除这一条；释放它占用的空间，请前往
      <button type="button" class="trash__link" @click="emit('navigate', 'special:gc')">
        仓库整理
      </button>。
    </p>
  </section>
</template>

<style scoped>
.trash {
  padding-top: 18px;
}

.trash__head {
  display: flex;
  gap: 12px;
  align-items: baseline;
}

.trash__title {
  margin: 0;
  font-size: 1.7em;
}

.trash__count {
  color: var(--text-dim);
  font-size: 13px;
}

.trash__lead {
  margin: 10px 0 18px;
  color: var(--text-dim);
  font-size: 0.95em;
  line-height: 1.7;
}

.trash__problem {
  margin: 12px 0;
  padding: 8px 12px;
  border-left: 3px solid var(--danger);
  color: var(--text);
  font-size: 13px;
}

.trash__hint {
  color: var(--text-dim);
  font-size: 13.5px;
}

.trash__list {
  margin: 0;
  padding: 0;
  list-style: none;
  border: 1px solid var(--border);
  border-radius: 8px;
  overflow: hidden;
}

.trash__item {
  display: flex;
  flex-wrap: wrap;
  gap: 4px 10px;
  align-items: center;
  padding: 10px 12px;
  border-top: 1px solid var(--border);
  font-size: 13.5px;
}

.trash__item:first-child {
  border-top: 0;
}

.trash__item:nth-child(odd) {
  background: var(--surface);
}

.trash__name {
  flex: 1 1 auto;
  min-width: 0;
  font-weight: 600;
  overflow-wrap: anywhere;
}

.trash__meta {
  color: var(--text-dim);
  font-size: 12.5px;
}

.trash__badge {
  padding: 1px 7px;
  border: 1px solid var(--border);
  border-radius: 999px;
  color: var(--text-dim);
  font-size: 11.5px;
}

.trash__actions {
  display: inline-flex;
  gap: 6px;
}

.trash__btn {
  display: inline-flex;
  gap: 5px;
  align-items: center;
  padding: 4px 10px;
  border: 1px solid var(--border);
  border-radius: 999px;
  background: transparent;
  color: var(--text-dim);
  font: inherit;
  font-size: 12.5px;
  cursor: pointer;
}

.trash__btn:hover:not(:disabled) {
  border-color: var(--accent-soft);
  background: var(--accent-tint);
  color: var(--text);
}

.trash__btn--danger:hover:not(:disabled) {
  border-color: var(--danger);
  background: transparent;
  color: var(--danger);
}

.trash__btn:disabled {
  opacity: 0.5;
  cursor: default;
}

.trash__footer {
  margin: 16px 0 0;
  color: var(--text-dim);
  font-size: 12.5px;
}

.trash__link {
  padding: 0;
  border: 0;
  background: transparent;
  color: var(--accent-soft);
  font: inherit;
  font-size: 12.5px;
  cursor: pointer;
}

.trash__link:hover {
  text-decoration: underline;
}
</style>
