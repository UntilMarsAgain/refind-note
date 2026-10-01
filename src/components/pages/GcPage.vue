<script setup lang="ts">
import { onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import {
  formatBytes,
  formatTime,
  type GcReport,
} from "../../ipc/maintenance.ts";
import { flash } from "../../core/notice.ts";
import { maintenance, refreshWorkspaceInfo } from "../../core/preferences.ts";

/**
 * 仓库整理（`special:gc`）。
 *
 * 回收两类**不再被引用**的东西：没有日志指着的内容块、没有主的草稿槽位。
 * 历史版本引用的内容**不会被回收** —— 包括回收站里那些笔记引用的：
 * 它们还会被还原，谁引用着谁就得在。
 *
 * 还有一件不能撤销的事单列一项：**先清空回收站**。它一勾，回收站里那些笔记就
 * 再也还原不回来了，而它们的内容块也才会变成孤儿、被顺手回收。
 */
const emit = defineEmits<{
  /** 去别的页面（去回收站看看） */
  (e: "navigate", input: string): void;
}>();

const orphanBlobs = ref(true);
const orphanDrafts = ref(true);
const purgeTrashFirst = ref(false);

const busy = ref(false);
const error = ref("");
const report = ref<GcReport | null>(null);

onMounted(() => void refreshWorkspaceInfo());

const canRun = () => !busy.value && (orphanBlobs.value || orphanDrafts.value || purgeTrashFirst.value);

async function run() {
  busy.value = true;
  error.value = "";
  report.value = null;

  try {
    // 先清回收站（如果勾了）：它是单独的一步，账单也分开报
    let purged = 0;
    if (purgeTrashFirst.value) {
      const result = await invoke<{ removed: number }>("purge_trash", {
        olderThanDays: 0,
      });
      purged = result.removed;
    }

    const gathered = await invoke<GcReport>("gc", {
      orphanBlobs: orphanBlobs.value,
      orphanDrafts: orphanDrafts.value,
    });
    report.value = gathered;
    flash(
      purged > 0
        ? `已清空回收站 ${purged} 条，并释放 ${formatBytes(gathered.freed_bytes)}`
        : `已释放 ${formatBytes(gathered.freed_bytes)}`,
    );
    await refreshWorkspaceInfo();
  } catch (reason) {
    error.value = String(reason);
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <section class="gc">
    <h1 class="gc__title">仓库整理</h1>

    <p class="gc__lead">
      整理并释放不再被任何版本引用的数据。<strong>历史版本仍在引用的内容不会被删除</strong>，
      回收站中笔记的内容同样保留 —— 还原它们时仍需要这些数据。
    </p>

    <ul class="gc__options">
      <li class="gc__option">
        <label>
          <input v-model="orphanBlobs" type="checkbox"/>
          <span>
            <strong>无引用的内容</strong>
            <em>不再被任何版本引用的笔记内容，通常来自已永久清除的笔记。</em>
          </span>
        </label>
      </li>
      <li class="gc__option">
        <label>
          <input v-model="orphanDrafts" type="checkbox"/>
          <span>
            <strong>无归属的草稿</strong>
            <em>草稿随笔记保存；笔记被永久清除后，这些草稿不会再被读取。</em>
          </span>
        </label>
      </li>
      <li class="gc__option">
        <label>
          <input v-model="purgeTrashFirst" type="checkbox"/>
          <span>
            <strong>先清空回收站</strong>
            <em>
              先永久删除回收站中的全部笔记，它们占用的内容随即一并释放。
              此操作<strong>不可撤销</strong>，这些笔记将无法再还原。
            </em>
          </span>
        </label>
      </li>
    </ul>

    <div class="gc__actions">
      <button type="button" class="gc__go" :disabled="!canRun()" @click="run">
        {{ busy ? "正在整理…" : "开始整理" }}
      </button>
      <span class="gc__warn">整理将删除上述数据，且无法撤销。</span>
    </div>

    <p v-if="error" class="gc__problem">{{ error }}</p>

    <div v-if="report" class="gc__report">
      <p>
        已释放 <strong>{{ formatBytes(report.freed_bytes) }}</strong>：
        内容 <strong>{{ report.removed_blobs }}</strong> 项、草稿
        <strong>{{ report.removed_drafts }}</strong> 份。
      </p>
    </div>

    <p class="gc__policy">
      自动整理每 <strong>{{ maintenance.gc_interval_days }}</strong>
      天执行一次，在启动时进行（上次执行：{{ formatTime(maintenance.last_gc) }}）。
      自动执行时<strong>只清理无引用的内容</strong>，不涉及草稿 ——
      草稿是否清理，由你在这里决定。执行间隔可在设置中调整。回收站：
      <button type="button" class="gc__link" @click="emit('navigate', 'special:trash')">
        查看
      </button>
    </p>
  </section>
</template>

<style scoped>
.gc {
  padding-top: 18px;
}

.gc__title {
  margin: 0;
  font-size: 1.7em;
}

.gc__lead {
  margin: 10px 0 18px;
  color: var(--text-dim);
  font-size: 0.95em;
  line-height: 1.7;
}

.gc__options {
  margin: 0;
  padding: 0;
  list-style: none;
  border: 1px solid var(--border);
  border-radius: 8px;
  overflow: hidden;
}

.gc__option {
  padding: 10px 12px;
  border-top: 1px solid var(--border);
}

.gc__option:first-child {
  border-top: 0;
}

.gc__option:nth-child(odd) {
  background: var(--surface);
}

.gc__option label {
  display: flex;
  gap: 10px;
  align-items: flex-start;
  cursor: pointer;
}

.gc__option input {
  margin-top: 4px;
  accent-color: var(--accent);
}

.gc__option strong {
  display: block;
  font-size: 13.5px;
  font-weight: 600;
}

.gc__option em {
  display: block;
  margin-top: 2px;
  color: var(--text-dim);
  font-size: 12.5px;
  font-style: normal;
  line-height: 1.7;
}

.gc__actions {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
  align-items: center;
  margin: 16px 0 0;
}

.gc__go {
  padding: 6px 14px;
  border: 1px solid var(--accent-soft);
  border-radius: 6px;
  background: transparent;
  color: var(--accent-soft);
  font: inherit;
  font-size: 13.5px;
  cursor: pointer;
}

.gc__go:hover:not(:disabled) {
  background: var(--accent);
  color: var(--text);
}

.gc__go:disabled {
  opacity: 0.5;
  cursor: default;
}

.gc__warn {
  color: var(--text-dim);
  font-size: 12.5px;
}

.gc__problem {
  margin: 14px 0 0;
  padding: 8px 12px;
  border-left: 3px solid var(--danger);
  color: var(--text);
  font-size: 13px;
}

.gc__report {
  margin: 16px 0 0;
  padding: 10px 12px;
  border-left: 3px solid var(--accent-soft);
  border-radius: 6px;
  background: var(--accent-tint);
  font-size: 13.5px;
}

.gc__report p {
  margin: 0;
}

.gc__policy {
  margin: 20px 0 0;
  color: var(--text-dim);
  font-size: 12.5px;
  line-height: 1.8;
}

.gc__link {
  padding: 0;
  border: 0;
  background: transparent;
  color: var(--accent-soft);
  font: inherit;
  font-size: 12.5px;
  cursor: pointer;
}

.gc__link:hover {
  text-decoration: underline;
}
</style>
