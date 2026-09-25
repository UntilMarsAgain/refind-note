<script setup lang="ts">
/**
 * 回收站（`special:trash`）。
 *
 * 列出删过的笔记，并提供"清理 30 天前的"。
 *
 * 两点刻意如此：
 * - **不做自动清理**：删数据是破坏性操作，应该有明确的触发者（写清策略、给按钮，而不是
 *   悄悄在后台删）；
 * - **如实说明还不能恢复**：这一页只做"看 + 清"，把"能做什么、不能做什么"讲明白，
 *   避免用户以为点了就有救。
 */
import { computed, onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";

interface TrashEntry {
  title: string;
  deleted_at: string;
  bytes: number;
  /** `null` = 删除时间读不出来（会列出，但不参与清理） */
  days_old: number | null;
}

interface GcReport {
  removed_blobs: number;
  freed_bytes: number;
  removed_drafts: number;
}

interface PurgeReport {
  removed: number;
  freed_bytes: number;
  blobs: GcReport;
}

/** 保留天数：超过它的条目才允许被清理 */
const KEEP_DAYS = 30;

const entries = ref<TrashEntry[]>([]);
const loading = ref(true);
const busy = ref(false);
const error = ref("");
const report = ref<PurgeReport | null>(null);

/**
 * 有多少条已经到可清理的年纪。
 *
 * 删除时间读不出来的条目不参与 —— 后端也不会删它们，两边的判断必须一致，
 * 否则按钮上写着 3 条、结果只清了 2 条。
 */
const expired = computed(
  () => entries.value.filter((item) => item.days_old !== null && item.days_old >= KEEP_DAYS),
);

/** 能否清理这一条（与后端同一套判断） */
function isExpired(item: TrashEntry): boolean {
  return item.days_old !== null && item.days_old >= KEEP_DAYS;
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
    return "（时间未知）";
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

async function purge() {
  busy.value = true;
  error.value = "";
  report.value = null;
  try {
    report.value = await invoke<PurgeReport>("purge_trash", {
      olderThanDays: KEEP_DAYS,
    });
    await refresh();
  } catch (reason) {
    error.value = String(reason);
  } finally {
    busy.value = false;
  }
}

onMounted(refresh);
</script>

<template>
  <section class="trash">
    <h1 class="trash__title">回收站</h1>
    <p class="trash__lead">
      删过的笔记都在这里，历史一条没丢。<strong>这里还没有恢复功能</strong>，
      只能查看与清理；要取回内容可以直接去 <code>~/.refind-note/trash/</code> 里找那个文件。
    </p>

    <p v-if="error" class="trash__error">{{ error }}</p>
    <p v-else-if="loading" class="trash__hint">正在读取…</p>

    <template v-else>
      <p v-if="entries.length === 0" class="trash__hint">回收站是空的。</p>

      <ul v-else class="trash__list">
        <li v-for="item in entries" :key="item.title" class="trash__item">
          <span class="trash__name">{{ item.title }}</span>
          <span class="trash__meta">
            {{ formatTime(item.deleted_at) }}
            <template v-if="item.days_old !== null"> · 已 {{ item.days_old }} 天</template>
            · {{ formatBytes(item.bytes) }}
          </span>
          <span v-if="isExpired(item)" class="trash__badge">可清理</span>
          <span v-else-if="item.days_old === null" class="trash__badge trash__badge--dim">
            时间未知，不清理
          </span>
        </li>
      </ul>

      <div v-if="entries.length > 0" class="trash__actions">
        <button
          class="trash__purge"
          type="button"
          :disabled="busy || expired.length === 0"
          @click="purge"
        >
          {{ busy ? "正在清理…" : `清理 ${KEEP_DAYS} 天前的（${expired.length} 条）` }}
        </button>
        <span class="trash__warn">清理会删除这些文件，无法撤销。</span>
      </div>

      <div v-if="report" class="trash__report">
        <p class="trash__report-row">
          清掉 {{ report.removed }} 条，释放 {{ formatBytes(report.freed_bytes) }}
        </p>
        <p class="trash__report-row">
          顺带回收内容块 {{ report.blobs.removed_blobs }} 个，释放
          {{ formatBytes(report.blobs.freed_bytes) }}
        </p>
      </div>
    </template>
  </section>
</template>

<style scoped>
.trash {
  max-width: 720px;
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

.trash__lead code {
  padding: 1px 5px;
  border-radius: 4px;
  background: var(--code-bg);
  font-size: 12px;
}

.trash__list {
  margin: 0;
  padding: 0;
  list-style: none;
}

.trash__item {
  display: flex;
  align-items: baseline;
  flex-wrap: wrap;
  gap: 8px;
  padding: 9px 0;
  border-bottom: 1px solid var(--border);
}

.trash__name {
  font-size: 14px;
}

.trash__meta {
  color: var(--text-dim);
  font-size: 12px;
}

.trash__badge--dim {
  color: var(--text-dim);
}

.trash__badge {
  padding: 0 6px;
  border-radius: 9px;
  background: var(--code-bg);
  color: var(--link-missing);
  font-size: 11px;
}

.trash__actions {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 12px;
  margin-top: 18px;
}

.trash__purge {
  padding: 6px 14px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background-color: transparent;
  color: var(--text);
  font-size: 13px;
  cursor: pointer;
}

.trash__purge:hover:not(:disabled) {
  background-color: var(--hover);
}

.trash__purge:disabled {
  opacity: 0.45;
  cursor: default;
}

.trash__warn {
  color: var(--text-dim);
  font-size: 12px;
}

.trash__hint {
  color: var(--text-dim);
  font-size: 13px;
}

.trash__error {
  color: var(--link-missing);
  font-size: 13px;
}

.trash__report {
  margin-top: 16px;
  padding: 10px 12px;
  border: 1px solid var(--border);
  border-radius: 8px;
  background: var(--surface);
}

.trash__report-row {
  margin: 2px 0;
  font-size: 13px;
}
</style>
