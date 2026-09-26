<script setup lang="ts">
import type { DiffResult, DiffLine, RevisionSummary } from "../bindings";
import { computed, onMounted, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { ArrowLeft } from "@lucide/vue";

/**
 * 版本历史 + 对比。
 *
 * 数据全来自后端（`note_history` / `compare_revisions` / `note_revision`）：
 * 差异算法已经在 Rust 侧算好并单元测试过，这里只负责折叠上下文与上色。
 */




const props = defineProps<{
  title: string;
  /** 当前版本号，用来在列表里标出「当前」 */
  currentRev: number;
  /** 地址栏写了「标题@版本」时，进来就选中这一版 */
  initialRev?: number | null;
}>();

const emit = defineEmits<{
  (e: "close"): void;
  /** 导航到某一版的地址（前端拼成「标题@缩写」交给统一的地址栏解析） */
  (e: "open-revision", reference: string): void;
  /** 回退到某一版：作为新提交写上去，旧记录不动 */
  /** 请求回退到某一版：带上缩写，交给统一的地址解析（先落到确认页） */
  (e: "rollback", reference: string): void;
}>();

/** 差异里每条改动前后保留的上下文行数 */
const CONTEXT = 3;

const history = ref<RevisionSummary[]>([]);
const loading = ref(false);
const error = ref("");
const selectedRev = ref<number | null>(null);
const diff = ref<DiffResult | null>(null);
/** 草稿默认不显示：它们是可被提交取代的临时节点，平时不是历史 */
const showDrafts = ref(false);

const visible = computed(() =>
  showDrafts.value
    ? history.value
    : history.value.filter((item) => item.kind !== "draft"),
);

/** 选中那一版的缩写 ID */
const selectedShort = computed(
  () => visible.value.find((item) => item.rev === selectedRev.value)?.short_id ?? "",
);

/** 折叠成片的 equal：只留改动行前后各 CONTEXT 行，其余折起来 */
const diffLines = computed(() => {
  const result = diff.value;
  if (!result) {
    return [] as DiffLine[];
  }

  const keep = new Set<number>();
  result.lines.forEach((line, index) => {
    if (line.kind === "equal") {
      return;
    }
    for (let offset = -CONTEXT; offset <= CONTEXT; offset += 1) {
      keep.add(index + offset);
    }
  });

  const out: DiffLine[] = [];
  let skipped = 0;
  const flush = () => {
    if (skipped > 0) {
      out.push({
        kind: "skip",
        old_line: null,
        new_line: null,
        text: `⋯ 省略 ${skipped} 行未改动的上下文`,
      });
      skipped = 0;
    }
  };

  result.lines.forEach((line, index) => {
    if (keep.has(index)) {
      flush();
      out.push(line);
    } else {
      skipped += 1;
    }
  });
  flush();

  return out;
});

function signOf(kind: string) {
  if (kind === "insert") {
    return "+";
  }
  if (kind === "delete") {
    return "−";
  }
  return kind === "skip" ? "⋯" : " ";
}

/** 后端给的是 ISO-8601 UTC；这里换成浏览器本地时间 */
function formatTime(value: string) {
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString();
}

async function load() {
  loading.value = true;
  error.value = "";
  try {
    history.value = await invoke<RevisionSummary[]>("note_history", {
      title: props.title,
    });
    // 地址栏指定了版本就选它，否则选最新一个有正文的版本
    const wanted = props.initialRev;
    const target =
      wanted && history.value.some((item) => item.rev === wanted)
        ? wanted
        : [...history.value].reverse().find((item) => item.rev > 0)?.rev;
    if (target) {
      await select(target);
    }
  } catch (err) {
    error.value = String(err);
  } finally {
    loading.value = false;
  }
}

/** 选中某一版：默认跟「列表里的上一条」比，也就是这一版改了什么 */
async function select(rev: number) {
  selectedRev.value = rev;

  const list = visible.value;
  const index = list.findIndex((item) => item.rev === rev);
  const previous = index > 0 ? list[index - 1] : undefined;

  // 第一条（创建）没有可比的对象
  if (!previous || previous.rev === 0) {
    diff.value = null;
    return;
  }

  try {
    diff.value = await invoke<DiffResult>("compare_revisions", {
      title: props.title,
      from: previous.rev,
      to: rev,
    });
  } catch (err) {
    error.value = String(err);
  }
}

onMounted(load);
watch(() => props.title, load);
</script>

<template>
  <section class="history">
    <header class="history__bar">
      <button class="hbtn" type="button" @click="emit('close')">
        <ArrowLeft :size="14" :stroke-width="1.9" />
        返回阅读
      </button>
      <h2 class="history__title">{{ title }} · 版本历史</h2>
      <label class="history__toggle">
        <input v-model="showDrafts" type="checkbox" />
        显示草稿
      </label>
    </header>

    <p v-if="error" class="history__error">{{ error }}</p>
    <p v-else-if="loading" class="history__hint">正在读取历史…</p>

    <div v-else class="history__body">
      <ol class="history__list">
        <li v-for="item in [...visible].reverse()" :key="item.rev">
          <button
            class="rentry"
            :class="{
              'rentry--active': selectedRev === item.rev,
              'rentry--draft': item.kind === 'draft',
            }"
            type="button"
            @click="item.rev > 0 && select(item.rev)"
          >
            <span class="rentry__rev">
              {{ item.kind === "create" ? "创建" : `版本 ${item.rev}` }}
            </span>
            <span class="rentry__summary">
              {{ item.summary || "（无摘要）" }}
            </span>
            <span class="rentry__meta">
              <code class="rentry__id">{{ item.short_id }}</code>
              · {{ formatTime(item.at) }}
              <template v-if="item.kind !== 'create'">
                · {{ item.bytes }} 字节
                <span v-if="item.delta > 0" class="rentry__plus">
                  +{{ item.delta }}
                </span>
                <span v-else-if="item.delta < 0" class="rentry__minus">
                  {{ item.delta }}
                </span>
              </template>
            </span>
            <span v-if="item.rev === currentRev" class="rentry__badge">当前</span>
            <span v-else-if="item.kind === 'draft'" class="rentry__badge">草稿</span>
          </button>
        </li>
      </ol>

      <div class="history__panel">
        <div class="history__panelbar">
          <span v-if="diff" class="history__stat">
            <span class="rentry__plus">+{{ diff.inserted }}</span>
            <span class="rentry__minus">−{{ diff.deleted }}</span>
            <span class="history__subjects">
              {{ diff.from_title }} → {{ diff.to_title }}
            </span>
          </span>

          <!-- 不在这里预览全文：导航到这一版的地址，由统一的地址栏解析负责显示 -->
          <button
            v-if="selectedShort"
            class="hbtn hbtn--accent"
            type="button"
            @click="emit('open-revision', selectedShort)"
          >
            打开这一版 @view-{{ selectedShort }}
          </button>

          <button
            v-if="selectedShort && selectedRev !== currentRev"
            class="hbtn"
            type="button"
            @click="emit('rollback', selectedShort)"
          >
            回退到这一版
          </button>
        </div>

        <!-- 注意：这里必须是真元素。裸 <template> 是惰性的，里面的内容不会渲染 -->
        <div class="history__diff">
          <p v-if="!diff" class="history__hint">这一版没有可比对的上一版。</p>
          <div v-else class="diff">
            <div
              v-for="(line, index) in diffLines"
              :key="index"
              class="diff__line"
              :class="`diff__line--${line.kind}`"
            >
              <span class="diff__no">{{ line.old_line ?? "" }}</span>
              <span class="diff__no">{{ line.new_line ?? "" }}</span>
              <span class="diff__sign">{{ signOf(line.kind) }}</span>
              <span class="diff__text">{{ line.text }}</span>
            </div>
          </div>
        </div>
      </div>
    </div>
  </section>
</template>

<style scoped>
.history {
  padding-top: 18px;
}

.history__bar {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
  align-items: center;
  margin-bottom: 12px;
}

.history__title {
  flex: 1 1 auto;
  margin: 0;
  font-size: 1.05em;
  font-weight: 600;
  line-height: 1.5;
}

.history__toggle {
  display: inline-flex;
  gap: 6px;
  align-items: center;
  color: var(--text-dim);
  font-size: 13px;
  cursor: pointer;
}

.history__body {
  display: flex;
  gap: 16px;
  align-items: flex-start;
}

.history__list {
  flex: 0 0 260px;
  max-height: 68vh;
  margin: 0;
  padding: 0;
  overflow-y: auto;
  list-style: none;
}

.history__panel {
  flex: 1 1 auto;
  min-width: 0;
}

.history__panelbar {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  align-items: center;
  margin-bottom: 8px;
}

.history__stat {
  display: inline-flex;
  gap: 8px;
  align-items: center;
  font-size: 12.5px;
}

.history__subjects {
  color: var(--text-dim);
}

.history__hint,
.history__error {
  margin: 0;
  padding: 8px 0;
  color: var(--text-dim);
  font-size: 13.5px;
}

.history__error {
  color: var(--link-missing);
}

.history__content {
  max-height: 68vh;
  padding: 12px 14px;
  overflow-y: auto;
  border: 1px solid var(--border);
  border-radius: 8px;
  background: var(--field-bg);
}

/* ---------- 列表项 ---------- */

.rentry {
  appearance: none;
  display: block;
  width: 100%;
  margin-bottom: 2px;
  padding: 7px 10px;
  border: 1px solid transparent;
  border-radius: 6px;
  background: transparent;
  color: var(--text-dim);
  font-size: 13px;
  line-height: 1.5;
  text-align: left;
  cursor: pointer;
}

.rentry:hover {
  background: var(--hover);
  color: var(--text);
}

.rentry--active {
  border-color: var(--border);
  background: var(--hover);
  color: var(--text);
}

.rentry--draft {
  /* 草稿是可被提交取代的临时节点，视觉上就压低一档 */
  opacity: 0.7;
}

.rentry__id {
  font-family: var(--mono-font);
  font-size: 11.5px;
  color: var(--text);
}

.rentry__rev {
  display: block;
  color: var(--text);
  font-weight: 600;
}

.rentry__summary {
  display: block;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.rentry__meta {
  display: block;
  font-size: 12px;
}

.rentry__badge {
  display: inline-block;
  margin-top: 3px;
  padding: 1px 6px;
  border: 1px solid var(--border);
  border-radius: 999px;
  font-size: 11px;
}

.rentry__plus {
  color: var(--syntax-string);
}

.rentry__minus {
  color: var(--link-missing);
}

/* ---------- 差异 ---------- */

.diff {
  max-height: 68vh;
  overflow: auto;
  border: 1px solid var(--border);
  border-radius: 8px;
  background: var(--bg);
  font-family: var(--mono-font);
  font-size: 13px;
  line-height: 1.6;
}

.diff__line {
  display: flex;
  gap: 8px;
  padding: 0 8px;
  white-space: pre-wrap;
  word-break: break-word;
}

.diff__line--insert {
  background: var(--diff-add-bg);
}

.diff__line--delete {
  background: var(--diff-del-bg);
}

.diff__line--equal {
  color: var(--text-dim);
}

.diff__line--skip {
  color: var(--text-dim);
  font-style: italic;
  opacity: 0.75;
}

.diff__no {
  flex: 0 0 2.4em;
  color: var(--text-dim);
  font-size: 11.5px;
  text-align: right;
  opacity: 0.7;
}

.diff__sign {
  flex: 0 0 1em;
  text-align: center;
}

.diff__line--insert .diff__sign {
  color: var(--syntax-string);
}

.diff__line--delete .diff__sign {
  color: var(--link-missing);
}

.diff__text {
  flex: 1 1 auto;
  min-width: 0;
}

/* ---------- 按钮 ---------- */

.hbtn {
  appearance: none;
  display: inline-flex;
  gap: 5px;
  align-items: center;
  height: 28px;
  padding: 0 10px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: transparent;
  color: var(--text-dim);
  font-size: 12.5px;
  line-height: 1.4;
  cursor: pointer;
  transition: background-color 120ms ease, color 120ms ease;
}

.hbtn:hover {
  background: var(--hover);
  color: var(--text);
}

.hbtn--on {
  border-color: var(--accent-soft);
  color: var(--accent-soft);
}

.hbtn--accent {
  border-color: var(--accent-soft);
  color: var(--accent-soft);
}

@media (max-width: 720px) {
  .history__body {
    flex-direction: column;
  }

  .history__list {
    flex: 1 1 auto;
    width: 100%;
    max-height: 32vh;
  }
}
</style>
