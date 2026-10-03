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
import { Search } from "@lucide/vue";
import { fuzzySearch } from "../../core/fuzzy.ts";
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
/** 顶部那排小按钮只摆**菜单里列的**那几页（列表里是全部，两者不是一回事） */
const menuHelp = ref<HelpPage[]>([]);
/**
 * 菜单里列哪几页 —— 与 Rust 侧 `features::help::PAGES` 一致。
 *
 * 为什么要在这儿再写一份：顶部那排小按钮摆的是"**常去**的那几页"，
 * 而列表里是"**全部**"。这个区别正是这一页该说的（见 `all_help_pages`），
 * 而后端 `PAGES` 是 `&[&str; 3]` 定长数组、命令也没把它送过来，
 * 所以这里写一份并用测试/Rust 侧盯着两处不漂。
 */
const MENU_PAGES = ["首页", "目录", "语法速览"];
const error = ref("");
const query = ref("");

/** 每页多少条 —— 一屏放得下、翻页也不至于太频繁 */
const PAGE_SIZE = 50;

/**
 * 列表里的一行：笔记与帮助页**混在一起**排。
 *
 * 混排而不是分两段，是因为这一页回答的是同一个问题：**这儿有哪些页面**。
 * 分两段等于让人先在心里做一次归类，而"帮助"和"笔记"并不是并列的两类东西 ——
 * 帮助页就是页面（它们有地址、能打开、有历史、能前进后退）。
 *
 * 所以统一成一个 `ListRow`：排序、筛选、分页都只看这一份。
 */
interface ListRow {
    /** 列表里的身份（笔记用 key，帮助页用 slug） */
    id: string;
    /** 显示名 */
    title: string;
    /** 打开它用的地址 */
    address: string;
    /** 那一行下面那行小字；没有就空 */
    meta: string;
    /** 是哪一类（只给样式与将来搜索用，不影响打开） */
    kind: "note" | "help";
}

/** 笔记与帮助合成一份列表（帮助在后 —— 它只有几页） */
const allRows = computed<ListRow[]>(() => [
    ...notes.value.map((note) => ({
        id: note.key,
        title: note.title,
        address: note.title,
        meta: `第 ${note.rev} 版 · ${note.bytes} 字节 · 修改于 ${note.modified}`,
        kind: "note" as const,
    })),
    ...help.value.map((page) => ({
        id: `help:${page.slug}`,
        title: page.display,
        address: `Help:${page.slug}`,
        meta: "随程序发布的帮助",
        kind: "help" as const,
    })),
]);

/**
 * 按查询筛出该显示的那些行。
 *
 * 打分与匹配的口径在 `core/fuzzy.ts` —— 那一层是"模糊搜索怎么算"的**唯一**出处，
 * 将来要换成库（`fuse.js` 之类）只改它一个文件，这个调用方一行不动。
 *
 * 空查询时**原样返回全部**（不排序）—— 没在找东西的时候，顺序就该是"本来的顺序"
 * （笔记按修改先后、帮助在后），而不是按某个查询词排出来的。
 */
const filteredRows = computed(() => {
    if (!query.value.trim()) {
        return allRows.value;
    }
    return fuzzySearch(query.value, allRows.value, (row) => row.title);
});

const pageCount = computed(() => Math.max(1, Math.ceil(filteredRows.value.length / PAGE_SIZE)));

/** 这一页要显示的那几条 */
const shownRows = computed(() =>
    filteredRows.value.slice((page.value - 1) * PAGE_SIZE, page.value * PAGE_SIZE),
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

/** 敲查询词时回到第一页：停在第 7 页而筛出 2 条，那一页是空的 */
function onQuery(event: Event) {
    query.value = (event.target as HTMLInputElement).value;
    page.value = 1;
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
        // 这里要的是**全部**帮助页，不是菜单里那三页（`help_pages`）：
        // 这一页回答的是"这儿有哪些页面"，清单不完整就是骗人。
        // 上面那排小按钮用的是另一份数据，仍是菜单那三页。
        const all = await invoke<HelpPage[]>("all_help_pages");
        help.value = all;
        // 顶部那排小按钮只摆菜单里列的那些（`menuHelp`），列表里是全部
        menuHelp.value = all.filter((page) => MENU_PAGES.includes(page.slug));
    } catch (reason) {
        console.warn("取帮助页清单失败：", reason);
    }
});
</script>

<template>
  <div class="all">
    <header class="all__head">
      <h1 class="all__title">全部页面</h1>
      <!-- 模糊搜索：筛下面那个列表（笔记与帮助页混在一起的那份）。
           放在标题下面而不是顶部悬浮，是因为这一页本来就是一张清单，
           筛选它不需要抢任何位置。 -->
      <div class="all__search">
        <Search class="all__search-icon" :size="15" :stroke-width="1.9" />
        <input
          class="all__search-input"
          type="search"
          placeholder="按标题筛选"
          aria-label="按标题筛选"
          :value="query"
          @input="onQuery"
        />
        <span v-if="query.trim()" class="all__search-count">
          {{ filteredRows.length }} 项
        </span>
      </div>
    </header>

    <!-- 顶部那排小按钮：**常去**的那几页入口（帮助是菜单里列的那几页）。
         它们是"去哪儿"的捷径；而下面的列表是全部页面（帮助也在里面）。
         两者并存是有意的 —— 一个是快捷方式，一个是清单。 -->
    <div v-if="menuHelp.length > 0 || pages.length > 0" class="all__jump">
      <div v-if="menuHelp.length > 0" class="all__jump-group">
        <h2 class="all__section all__section--inline">帮助</h2>
        <button
            v-for="page in menuHelp"
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
      {{ query.trim() ? '筛选结果' : '全部' }}
      <span v-if="filteredRows.length > 0" class="all__range">
        第 {{ (page - 1) * PAGE_SIZE + 1 }}–{{ Math.min(page * PAGE_SIZE, filteredRows.length) }} 项，共 {{ filteredRows.length }} 项
      </span>
    </h2>
    <p v-if="error" class="all__error">{{ error }}</p>
    <p v-else-if="allRows.length === 0" class="all__hint">
      仓库中暂无笔记，可在地址栏输入名称创建。
    </p>
    <p v-else-if="filteredRows.length === 0" class="all__hint">
      没有标题里带「{{ query }}」的页面。
    </p>
    <template v-else>
      <ol class="all__list">
        <li v-for="row in shownRows" :key="row.id">
          <button type="button" class="all__item" @click="emit('navigate', row.address)">
            <span class="all__name">
              <!-- 帮助页摆成 `Help:首页` 那样，与地址栏里写的一致，
                   免得"列表里这么写、地址里那么写"要人自己对一次 -->
              <span class="all__tag" :class="`all__tag--${row.kind}`">
                {{ row.kind === "help" ? "帮助" : "笔记" }}
              </span>
              {{ row.title }}
            </span>
            <span class="all__meta">{{ row.meta }}</span>
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

/* ---- 顶部那个筛选框 ---- */

.all__search {
  display: flex;
  gap: 8px;
  align-items: center;
  margin-top: 12px;
}

.all__search-icon {
  flex: 0 0 auto;
  color: var(--text-dim);
}

.all__search-input {
  flex: 1;
  /* 别太宽：这一页窄窗口也常开着，太宽会让输入框与下面列表的右边缘差很远 */
  max-width: 420px;
  height: 32px;
  padding: 0 10px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: var(--field-bg);
  color: var(--text);
  font: inherit;
  font-size: 13.5px;
}

.all__search-input:focus {
  outline: none;
  border-color: var(--accent-soft);
}

/* 命中多少项：等宽数字，改字时不抖 */
.all__search-count {
  color: var(--text-dim);
  font-size: 12.5px;
  font-variant-numeric: tabular-nums;
}

/* ---- 列表里那一行的类别标签 ---- */

.all__tag {
  display: inline-block;
  margin-right: 7px;
  padding: 0 6px;
  border-radius: 4px;
  font-size: 11px;
  vertical-align: 1px;
}

/* 帮助页标成"帮助" —— 不是另立一类，是标出这一页从哪儿来 */
.all__tag--help {
  background: var(--accent-tint);
  color: var(--accent);
}

.all__tag--note {
  background: var(--hover);
  color: var(--text-dim);
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
