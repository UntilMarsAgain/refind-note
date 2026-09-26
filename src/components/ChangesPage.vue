<script setup lang="ts">
/**
 * `special:changes` —— 最近更改。
 *
 * 把各篇笔记的版本记录汇到一起、按时间倒序。默认**不看草稿**：草稿随时在落盘，
 * 混进来会把"谁提交了什么"淹掉 —— 而这一页要回答的正是那个问题。
 */
import { onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { openMenu } from "../context-menu";
import { Clock } from "@lucide/vue";
import type { ChangeEntry } from "../bindings";

const emit = defineEmits<{
  (e: "open", address: string): void;
  /** Ctrl/Cmd 点击，或者右键菜单里选「在新标签页打开」 */
  (e: "open-new-tab", address: string): void;
}>();

/** 一次看多少条。给个上限是因为"最近"本来就该有边界 */
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
  } catch (error) {
    problem.value = String(error);
  } finally {
    busy.value = false;
  }
}

/** 这一条对应的地址：能定位到那一版就定位过去，否则就是这一页 */
function addressOf(entry: ChangeEntry): string {
  return entry.short_id ? entry.title + "@view-" + entry.short_id : entry.title;
}

/** 点一条：Ctrl/Cmd 点击＝在新标签页打开（与别处一个规矩） */
function open(event: MouseEvent, entry: ChangeEntry) {
  if (event.ctrlKey || event.metaKey) {
    emit("open-new-tab", addressOf(entry));
    return;
  }
  emit("open", addressOf(entry));
}

/** 条目上右键：与别处一致 */
function onRowMenu(event: MouseEvent, entry: ChangeEntry) {
  event.preventDefault();
  event.stopPropagation();
  const address = addressOf(entry);
  openMenu(event, [
    { label: "在新标签页打开", run: () => emit("open-new-tab", address) },
    { label: "复制地址", run: () => writeText(address) },
  ]);
}

function when(at: string): string {
  return at ? new Date(at).toLocaleString() : "";
}

/** 字节增减：删除没有"改了多少"可言，留空 */
function delta(entry: ChangeEntry): string {
  if (entry.kind === "delete") {
    return "";
  }
  return entry.delta > 0 ? "+" + entry.delta : String(entry.delta);
}

onMounted(refresh);
</script>

<template>
  <section class="changes">
    <header class="changes__head">
      <div class="changes__lead">
        <Clock :size="20" />
        <span>{{ entries.length }} 条记录</span>
      </div>
      <div class="changes__tools">
        <label class="changes__check">
          <input v-model="showDrafts" type="checkbox" @change="refresh" />
          <span>包含草稿</span>
        </label>
        <button type="button" :disabled="busy" @click="refresh">
          {{ busy ? "读取中…" : "刷新" }}
        </button>
      </div>
    </header>

    <p class="changes__hint">
      各篇笔记的版本记录汇总，按时间倒序。点一条可以去看**那一版**；
      草稿时刻都在落盘，所以默认不显示 —— 要看正在进行中的改动，勾上左边那一项。
    </p>

    <p v-if="problem" class="changes__problem">{{ problem }}</p>

    <p v-if="entries.length === 0" class="changes__empty">这段时间里没有更改。</p>

    <ul v-else class="changes__list">
      <li v-for="entry in entries" :key="entry.title + entry.rev" class="changes__item">
        <button
          class="changes__open"
          type="button"
          @click="open($event, entry)"
          @contextmenu="onRowMenu($event, entry)"
        >
          <span class="changes__when">{{ when(entry.at) }}</span>
          <span class="changes__kind" :class="'changes__kind--' + entry.kind">
            {{ KIND_LABELS[entry.kind] ?? entry.kind }}
          </span>
          <span class="changes__title">{{ entry.title }}</span>
          <span class="changes__delta">{{ delta(entry) }}</span>
          <span class="changes__id">{{ entry.short_id }}</span>
        </button>
      </li>
    </ul>
  </section>
</template>

<style scoped>
.changes {
  padding: 4px 0 32px;
}

.changes__head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 10px;
}

.changes__lead {
  display: flex;
  align-items: center;
  gap: 8px;
  color: var(--text-dim);
}

.changes__tools {
  display: flex;
  align-items: center;
  gap: 12px;
}

.changes__check {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  color: var(--text-dim);
  cursor: pointer;
}

.changes__tools button {
  padding: 5px 10px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: var(--bg);
  color: var(--text);
  cursor: pointer;
}

.changes__tools button:disabled {
  opacity: 0.5;
  cursor: default;
}

.changes__hint,
.changes__empty {
  color: var(--text-dim);
  font-size: 0.92em;
  line-height: 1.7;
}

.changes__problem {
  color: var(--danger);
}

.changes__list {
  margin: 0;
  padding: 0;
  list-style: none;
  display: flex;
  flex-direction: column;
}

.changes__open {
  width: 100%;
  display: grid;
  grid-template-columns: 11em 3.5em 1fr 4em 5em;
  gap: 10px;
  align-items: baseline;
  padding: 8px 4px;
  border: 0;
  border-bottom: 1px solid var(--border);
  background: none;
  color: var(--text);
  text-align: left;
  cursor: pointer;
  font-size: 0.95em;
}

.changes__open:hover {
  background: var(--accent-tint);
}

.changes__when,
.changes__id,
.changes__delta {
  color: var(--text-dim);
  font-size: 0.88em;
}

.changes__id {
  font-family: var(--mono, monospace);
}

.changes__delta {
  text-align: right;
}

/* 草稿用淡一点的色：它是进行中的东西，不该和已提交的看起来一样重 */
.changes__kind {
  color: var(--text-dim);
}

.changes__kind--commit {
  color: var(--link-blue);
}

.changes__kind--delete {
  color: var(--danger);
}

.changes__title {
  word-break: break-all;
}
</style>
