<script setup lang="ts">
import { onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { Clock } from "@lucide/vue";
import type { ChangeEntry } from "../../bindings/activity.ts";

/**
 * 最近编辑（`special:changes`）：**跨全部笔记**的版本流水。
 *
 * 一条记录是**一次提交**（不是一篇笔记），按时间倒序。点一条去看那一版。
 * 草稿是每篇一个槽位、随时会被覆盖，所以默认不列 —— 想看进行中的改动就勾上。
 */
const emit = defineEmits<{
  /** 去看某一版（地址由上层拼） */
  (e: "open", input: string): void;
}>();

/** 一次列这么多条：再多就该用别的方式找了 */
const LIMIT = 100;

const entries = ref<ChangeEntry[]>([]);
const showDrafts = ref(false);
const busy = ref(false);
const problem = ref("");

const KIND_LABELS: Record<string, string> = {
  commit: "提交",
  draft: "草稿",
  delete: "删除",
};

async function refresh() {
  busy.value = true;
  problem.value = "";
  try {
    entries.value = await invoke<ChangeEntry[]>("recent_changes", {
      limit: LIMIT,
      includeDrafts: showDrafts.value,
    });
  } catch (reason) {
    entries.value = [];
    problem.value = String(reason);
  } finally {
    busy.value = false;
  }
}

onMounted(() => void refresh());

function when(at: string): string {
  if (!at) {
    return "";
  }
  const stamp = new Date(at);
  return Number.isNaN(stamp.getTime()) ? at : stamp.toLocaleString();
}

function kindLabel(kind: string): string {
  return KIND_LABELS[kind] ?? kind;
}

/** 点一条：去看**那一版**（草稿没有版本，去看笔记本身） */
function open(entry: ChangeEntry) {
  emit("open", entry.kind === "commit" ? `${entry.title}@view-${entry.rev}` : entry.title);
}

/** 字节增减：删除没有内容，草稿不跟上一版比 */
function deltaOf(entry: ChangeEntry): string {
  if (entry.kind !== "commit") {
    return "";
  }
  return entry.delta > 0 ? `+${entry.delta}` : String(entry.delta);
}
</script>

<template>
  <section class="changes">
    <div class="changes__head">
      <Clock :size="20" :stroke-width="1.9"/>
      <h1 class="changes__title">最近编辑</h1>
      <span class="changes__count">{{ entries.length }} 条记录</span>

      <label class="changes__toggle">
        <input v-model="showDrafts" type="checkbox" @change="refresh"/>
        包含草稿
      </label>
      <button class="changes__refresh" type="button" :disabled="busy" @click="refresh">
        {{ busy ? "读取中…" : "刷新" }}
      </button>
    </div>

    <p class="changes__lead">
      全部笔记的版本记录，按时间倒序排列。点击可查看<strong>对应版本</strong>。
      草稿会被后续修改覆盖，因此默认不列出；如需查看进行中的改动，请勾选「包含草稿」。
    </p>

    <p v-if="problem" class="changes__problem">{{ problem }}</p>
    <p v-else-if="entries.length === 0" class="changes__hint">暂无改动记录。</p>

    <ol v-else class="changes__list">
      <li v-for="(entry, index) in entries" :key="`${entry.title}-${entry.rev}-${index}`">
        <button type="button" class="changes__open" @click="open(entry)">
          <span class="changes__at">{{ when(entry.at) }}</span>
          <span class="changes__kind" :class="`changes__kind--${entry.kind}`">
            {{ kindLabel(entry.kind) }}
          </span>
          <span class="changes__name">{{ entry.title }}</span>
          <span v-if="entry.kind === 'commit'" class="changes__rev">第 {{ entry.rev }} 版</span>
          <span class="changes__delta">{{ deltaOf(entry) }}</span>
        </button>
      </li>
    </ol>
  </section>
</template>

<style scoped>
.changes {
  padding-top: 18px;
}

.changes__head {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
  align-items: center;
}

.changes__title {
  margin: 0;
  font-size: 1.7em;
}

.changes__count {
  color: var(--text-dim);
  font-size: 13px;
}

.changes__toggle {
  display: inline-flex;
  gap: 6px;
  align-items: center;
  margin-left: auto;
  color: var(--text-dim);
  font-size: 12.5px;
  cursor: pointer;
}

.changes__toggle input {
  accent-color: var(--accent);
}

.changes__refresh {
  padding: 4px 12px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: transparent;
  color: var(--text-dim);
  font: inherit;
  font-size: 12.5px;
  cursor: pointer;
}

.changes__refresh:hover:not(:disabled) {
  border-color: var(--accent-soft);
  color: var(--text);
}

.changes__refresh:disabled {
  opacity: 0.6;
  cursor: default;
}

.changes__lead {
  margin: 10px 0 16px;
  color: var(--text-dim);
  font-size: 0.95em;
  line-height: 1.75;
}

.changes__problem {
  margin: 12px 0;
  padding: 8px 12px;
  border-left: 3px solid var(--danger);
  color: var(--text);
  font-size: 13px;
}

.changes__hint {
  color: var(--text-dim);
  font-size: 13.5px;
}

.changes__list {
  margin: 0;
  padding: 0;
  list-style: none;
  border-top: 1px solid var(--border);
}

.changes__open {
  display: grid;
  grid-template-columns: 11em 3.5em minmax(0, 1fr) 4.5em 4em;
  gap: 10px;
  align-items: baseline;
  width: 100%;
  padding: 8px 8px;
  border: 0;
  border-bottom: 1px solid var(--border);
  background: transparent;
  color: var(--text);
  font: inherit;
  font-size: 13.5px;
  text-align: left;
  cursor: pointer;
}

.changes__open:hover {
  background: var(--accent-tint);
}

.changes__at {
  color: var(--text-dim);
  font-size: 12.5px;
}

.changes__kind {
  font-size: 12.5px;
}

.changes__kind--commit {
  color: var(--link-blue);
}

.changes__kind--delete {
  color: var(--danger);
}

.changes__kind--draft {
  color: var(--text-dim);
}

.changes__name {
  overflow-wrap: anywhere;
}

.changes__rev,
.changes__delta {
  color: var(--text-dim);
  font-size: 12.5px;
  text-align: right;
}
</style>
