<script lang="ts">
import { ref } from "vue";

/**
 * 列表停在第几页（1 起）。
 *
 * 放在**模块级**（而不是 `setup` 里）：翻到第几页是"看这份列表时人在哪儿"，
 * 点进一篇再后退回来该还站在那一页。它不进地址 —— 地址里加个页码，后退键就变成
 * "上一个页码"了，而这里要的只是回到刚才站的地方。
 */
const page = ref(1);
</script>

<script setup lang="ts">
import { computed, onMounted } from "vue";
import { invoke } from "@tauri-apps/api/core";
import type { HelpPage } from "../../ipc/help.ts";
import type { NoteSummary } from "../../ipc/note.ts";
import { metaOf } from "../../core/special.ts";

/**
 * 全部页面：仓库里现有的笔记 + 特殊页面（＋随程序发布的帮助页）。
 *
 * 三样都在这一页上，因为这一页回答的是同一个问题：**这儿有哪些页面**。
 *
 * 笔记**分页**：一个用得久的仓库很容易上千篇，一路滚下去既慢又找不着北；
 * 一页一页翻反而知道自己在哪儿。帮助与特殊页面各只有几行，一屏放得下，不参与分页。
 */
const emit = defineEmits<{
    (e: "navigate", title: string): void;
}>();

const notes = ref<NoteSummary[]>([]);
const pages = ref<string[]>([]);
const help = ref<HelpPage[]>([]);
const error = ref("");

/** 每页多少条 —— 一屏放得下、翻页也不至于太频繁 */
const PAGE_SIZE = 50;

const pageCount = computed(() => Math.max(1, Math.ceil(notes.value.length / PAGE_SIZE)));

/** 这一页要显示的那几条 */
const shownNotes = computed(() =>
    notes.value.slice((page.value - 1) * PAGE_SIZE, page.value * PAGE_SIZE),
);

/**
 * 翻页按钮上要摆哪几页。
 *
 * 页数少就全摆；多了就在当前页左右各留两页，两头用 `null` 表示省略：
 * 一排几十个页码与滚动条一样没用。
 */
const pageButtons = computed<(number | null)[]>(() => {
    const total = pageCount.value;
    if (total <= 9) {
        return Array.from({ length: total }, (_, index) => index + 1);
    }
    const around = new Set([1, total]);
    for (let at = page.value - 2; at <= page.value + 2; at += 1) {
        if (at >= 1 && at <= total) {
            around.add(at);
        }
    }
    const sorted = [...around].sort((a, b) => a - b);
    const out: (number | null)[] = [];
    let previous = 0;
    for (const value of sorted) {
        if (previous && value - previous > 1) {
            out.push(null);
        }
        out.push(value);
        previous = value;
    }
    return out;
});

/** 翻到某一页（越界就不动） */
function goTo(target: number) {
    page.value = Math.min(Math.max(1, target), pageCount.value);
}

onMounted(async () => {
    try {
        notes.value = await invoke<NoteSummary[]>("list_notes");
        // 仓库可能比上次小（删了笔记）：停在超出范围的那一页会看到一片空白
        goTo(page.value);
    } catch (reason) {
        error.value = String(reason);
    }

    try {
        pages.value = (await invoke<string[]>("special_pages")).filter((page) => page !== "newtab");
    } catch (reason) {
        console.warn("取特殊页面清单失败：", reason);
    }

    try {
        help.value = await invoke<HelpPage[]>("help_pages");
    } catch (reason) {
        console.warn("取帮助页清单失败：", reason);
    }
});
</script>

<template>
  <div class="all">
    <header class="all__head">
      <h1 class="all__title">全部页面</h1>
    </header>

    <!-- 帮助与特殊页面：都只有几个入口，摆成一排小按钮 —— 它们是"去哪儿"，
         不是内容本身。按行铺开会把笔记（这一页真正要看的东西）挤到屏幕外面去。 -->
    <div v-if="help.length > 0 || pages.length > 0" class="all__jump">
      <div v-if="help.length > 0" class="all__jump-group">
        <h2 class="all__section all__section--inline">帮助</h2>
        <button
            v-for="page in help"
            :key="page.slug"
            type="button"
            class="all__chip"
            :title="page.display"
            @click="emit('navigate', `Help:${page.slug}`)"
        >
          {{ page.slug }}
        </button>
      </div>

      <div v-if="pages.length > 0" class="all__jump-group">
        <h2 class="all__section all__section--inline">特殊页面</h2>
        <button
            v-for="page in pages"
            :key="page"
            type="button"
            class="all__chip"
            :title="metaOf(page).tip"
            @click="emit('navigate', `special:${page}`)"
        >
          {{ metaOf(page).label }}
        </button>
      </div>
    </div>

    <h2 class="all__section">
      笔记
      <span v-if="notes.length > 0" class="all__range">
        第 {{ (page - 1) * PAGE_SIZE + 1 }}–{{ Math.min(page * PAGE_SIZE, notes.length) }} 篇，共 {{ notes.length }} 篇
      </span>
    </h2>
    <p v-if="error" class="all__error">{{ error }}</p>
    <p v-else-if="notes.length === 0" class="all__hint">
      仓库中暂无笔记，可在地址栏输入名称创建。
    </p>
    <template v-else>
      <ol class="all__list">
        <li v-for="note in shownNotes" :key="note.key">
          <button type="button" class="all__item" @click="emit('navigate', note.title)">
            <span class="all__name">{{ note.title }}</span>
            <span class="all__meta">第 {{ note.rev }} 版 · {{ note.bytes }} 字节 · 修改于 {{ note.modified }}</span>
          </button>
        </li>
      </ol>

      <!-- 只有一页时不摆页码：那时它只是几个点不动的按钮 -->
      <nav v-if="pageCount > 1" class="all__pager">
        <button
            type="button"
            class="all__page all__page--step"
            :disabled="page <= 1"
            @click="goTo(page - 1)"
        >
          上一页
        </button>

        <template v-for="(value, index) in pageButtons" :key="`${value}-${index}`">
          <span v-if="value === null" class="all__gap">…</span>
          <button
              v-else
              type="button"
              class="all__page"
              :class="{ 'all__page--on': value === page }"
              :aria-current="value === page ? 'page' : undefined"
              @click="goTo(value)"
          >
            {{ value }}
          </button>
        </template>

        <button
            type="button"
            class="all__page all__page--step"
            :disabled="page >= pageCount"
            @click="goTo(page + 1)"
        >
          下一页
        </button>
      </nav>
    </template>
  </div>
</template>

<style scoped>
.all__head {
  padding: 28px 0 8px;
}

.all__title {
  margin: 0;
  font-size: 26px;
  font-weight: 600;
  line-height: 1.35;
}

/* 帮助 / 特殊页面：一行一组，按钮自己换行 */
.all__jump {
  display: flex;
  flex-direction: column;
  gap: 6px;
  margin-top: 18px;
}

.all__jump-group {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  align-items: center;
}

.all__chip {
  padding: 4px 11px;
  border: 1px solid var(--border);
  border-radius: 999px;
  background: transparent;
  color: var(--text-dim);
  font: inherit;
  font-size: 12.5px;
  cursor: pointer;
}

.all__chip:hover {
  border-color: var(--accent-soft);
  background: var(--accent-tint);
  color: var(--text);
}

.all__section {
  display: flex;
  gap: 10px;
  align-items: baseline;
  margin: 22px 0 0;
  padding-bottom: 6px;
  border-bottom: 1px solid var(--border);
  color: var(--text-dim);
  font-size: 14px;
  font-weight: 500;
}

.all__range {
  color: var(--text-dim);
  font-size: 12px;
  font-weight: 400;
}

/* 紧凑那一组里的标题：只是个行首的标签，不要下划线与上下留白 */
.all__section--inline {
  margin: 0 6px 0 0;
  padding: 0;
  border-bottom: 0;
  font-size: 12.5px;
  white-space: nowrap;
}

/* 翻页 */
.all__pager {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
  align-items: center;
  margin-top: 14px;
}

.all__page {
  min-width: 30px;
  height: 28px;
  padding: 0 9px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: transparent;
  color: var(--text-dim);
  font: inherit;
  font-size: 12.5px;
  cursor: pointer;
}

.all__page:hover:not(:disabled) {
  border-color: var(--accent-soft);
  color: var(--text);
}

.all__page--on {
  border-color: var(--accent-soft);
  background: var(--accent-tint);
  color: var(--text);
  font-weight: 600;
}

.all__page:disabled {
  opacity: 0.4;
  cursor: default;
}

.all__gap {
  padding: 0 2px;
  color: var(--text-dim);
}

.all__list {
  margin: 8px 0 0;
  padding: 0;
  list-style: none;
  border-top: 1px solid var(--border);
}

.all__item {
  display: block;
  width: 100%;
  padding: 10px 8px;
  border: 0;
  border-bottom: 1px solid var(--border);
  background: transparent;
  color: var(--text);
  font: inherit;
  text-align: left;
  cursor: pointer;
}

.all__item:hover {
  background: var(--hover);
}

.all__name {
  display: block;
  font-size: 15px;
}

.all__meta {
  display: block;
  margin-top: 2px;
  color: var(--text-dim);
  font-size: 12.5px;
}

.all__hint,
.all__error {
  margin: 28px 0 0;
  color: var(--text-dim);
  font-size: 13.5px;
}

.all__error {
  color: var(--danger);
}
</style>
