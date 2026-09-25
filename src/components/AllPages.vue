<script setup lang="ts">
/**
 * 全部页面（`special:all`）：列出数据库里所有笔记，以及所有可用的特殊页面。
 *
 * 「有哪些特殊页面」**以后端为准**（`special_pages` 命令），前端只决定怎么显示它们；
 * 否则每加一个特殊页面都要在前端再登记一次，那就成了两个真相。
 */
import { computed, onMounted, ref } from "vue";
import { labelOf } from "../special";
import { invoke } from "@tauri-apps/api/core";

interface NoteSummary {
  key: string;
  title: string;
  /** 指令页面的短名；普通页面是 null */
  command: string | null;
}

/**
 * 每页最多显示多少篇。
 *
 * 所有页面共用"一篇一行"的列表，所以这一个数就是分页的全部依据；
 * 页号是**地址的一部分**（`special:all#3`），因此刷新、前进后退、分享地址都能落到同一页。
 */
const PAGE_SIZE = 50;

/** 指令页面的标记文案；没登记的退回短名本身 */
const COMMAND_LABELS: Record<string, string> = {
  redirect: "重定向",
  "random-redirect": "随机重定向",
  unrecognized: "指令有问题",
};

const emit = defineEmits<{
  (e: "open", address: string): void;
  (e: "open-new", address: string): void;
  (e: "page", page: number): void;
}>();

const props = defineProps<{
  /**
   * 地址里的 `#…`。
   *
   * 对 `special:all` 来说它只有一个含义：**页号**（`special:all#3` 是第 3 页）。
   * 别的特殊页面（如设置页）把这个位置用作条目锚点，那是各自的约定。
   */
  pageSection?: string;
}>();

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

/** 当前页号：地址里没写、或写了不是正整数的东西，都当第 1 页 */
const page = computed(() => {
  const parsed = Number.parseInt(props.pageSection ?? "", 10);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : 1;
});

const totalPages = computed(() =>
  Math.max(1, Math.ceil(notes.value.length / PAGE_SIZE)),
);

/**
 * 页号越界时**只夹住显示**，不去改地址。
 *
 * 地址是权威，改地址属于导航（由上层决定）；组件擅自替换地址，
 * 就会出现"看着在第 3 页、地址却是 #9"这种两个真相。
 */
const safePage = computed(() => Math.min(page.value, totalPages.value));

const pageNotes = computed(() =>
  notes.value.slice((safePage.value - 1) * PAGE_SIZE, safePage.value * PAGE_SIZE),
);

/** 跳页输入框的草稿：回车或点「跳转」才生效 */
const pageDraft = ref("");

function jumpToPage() {
  const parsed = Number.parseInt(pageDraft.value, 10);
  if (!Number.isFinite(parsed)) {
    return;
  }
  pageDraft.value = "";
  emit("page", Math.min(Math.max(1, parsed), totalPages.value));
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
        还没有笔记。可在 <code>special:newtab</code> 新建。
      </p>
      <ul v-else class="all__list">
        <li v-for="note in pageNotes" :key="note.key">
          <button
            class="all__link"
            type="button"
            :title="note.key"
            @click="go(note.title, $event)"
          >
            <span class="all__name">{{ note.title }}</span>
            <span
              v-if="note.command"
              class="all__cmd"
              :class="{ 'all__cmd--bad': note.command === 'unrecognized' }"
            >{{ COMMAND_LABELS[note.command] ?? note.command }}</span>
          </button>
        </li>
      </ul>

      <!-- 分页：页号就是地址里的 #，所以翻页也是一次导航（进历史，可后退） -->
      <nav v-if="notes.length > 0" class="all__pager">
        <button
          class="all__page"
          type="button"
          :disabled="safePage <= 1"
          @click="emit('page', safePage - 1)"
        >
          上一页
        </button>

        <span class="all__page-state">
          第
          <input
            v-model="pageDraft"
            class="all__page-input"
            type="text"
            inputmode="numeric"
            :placeholder="String(safePage)"
            @keydown.enter.prevent="jumpToPage"
          />
          / {{ totalPages }} 页
        </span>

        <button class="all__page" type="button" @click="jumpToPage">跳转</button>

        <button
          class="all__page"
          type="button"
          :disabled="safePage >= totalPages"
          @click="emit('page', safePage + 1)"
        >
          下一页
        </button>

        <span class="all__page-total">
          共 {{ notes.length }} 篇，每页 {{ PAGE_SIZE }} 篇
        </span>
      </nav>

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
  display: flex;
  align-items: center;
  gap: 6px;
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

/* 标题省略号只作用在标题上，标记不被压缩 */
.all__name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.all__cmd {
  flex-shrink: 0;
  padding: 0 6px;
  border-radius: 9px;
  background: var(--code-bg);
  color: var(--text-dim);
  font-size: 11px;
}

/* 认不出的指令页面一打开就报错，用缺失链接的红色提醒 */
.all__cmd--bad {
  color: var(--link-missing);
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

/* ---------- 分页 ---------- */

.all__pager {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
  margin: 14px 0 4px;
  padding-top: 10px;
  border-top: 1px solid var(--border);
}

.all__page {
  padding: 4px 10px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background-color: transparent;
  color: var(--text-dim);
  font-size: 12px;
  cursor: pointer;
}

.all__page:hover:not(:disabled) {
  background-color: var(--hover);
  color: var(--text);
}

.all__page:disabled {
  opacity: 0.45;
  cursor: default;
}

.all__page-state {
  display: flex;
  align-items: center;
  gap: 4px;
  color: var(--text-dim);
  font-size: 12px;
}

.all__page-input {
  width: 52px;
  padding: 3px 6px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: var(--field-bg);
  color: var(--text);
  font-size: 12px;
  text-align: center;
}

.all__page-total {
  margin-left: auto;
  color: var(--text-dim);
  font-size: 12px;
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
