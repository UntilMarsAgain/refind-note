<script setup lang="ts">
import { onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import type { HelpPage } from "../../bindings/help.ts";
import type { NoteSummary } from "../../bindings/note.ts";
import { metaOf } from "../../core/special.ts";

/**
 * 全部页面：仓库里现有的笔记 + 特殊页面（＋随程序发布的帮助页）。
 *
 * 三样都在这一页上，因为这一页回答的是同一个问题：**这儿有哪些页面**。
 */
const emit = defineEmits<{
    (e: "navigate", title: string): void;
}>();

const notes = ref<NoteSummary[]>([]);
const pages = ref<string[]>([]);
const help = ref<HelpPage[]>([]);
const error = ref("");

onMounted(async () => {
    try {
        notes.value = await invoke<NoteSummary[]>("list_notes");
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
      <p class="all__count">共 {{ notes.length }} 篇</p>
    </header>

    <h2 v-if="help.length > 0" class="all__section">帮助</h2>
    <ol v-if="help.length > 0" class="all__list">
      <li v-for="page in help" :key="page.slug">
        <button type="button" class="all__item" @click="emit('navigate', `Help:${page.slug}`)">
          <span class="all__name">{{ page.display }}</span>
        </button>
      </li>
    </ol>

    <h2 v-if="pages.length > 0" class="all__section">特殊页面</h2>
    <ol v-if="pages.length > 0" class="all__list">
      <li v-for="page in pages" :key="page">
        <button type="button" class="all__item" @click="emit('navigate', `special:${page}`)">
          <span class="all__name">{{ metaOf(page).label }}</span>
          <span class="all__meta">{{ metaOf(page).tip }}</span>
        </button>
      </li>
    </ol>

    <h2 class="all__section">笔记</h2>
    <p v-if="error" class="all__error">{{ error }}</p>
    <p v-else-if="notes.length === 0" class="all__hint">
      仓库中暂无笔记，可在地址栏输入名称创建。
    </p>
    <ol v-else class="all__list">
      <li v-for="note in notes" :key="note.key">
        <button type="button" class="all__item" @click="emit('navigate', note.title)">
          <span class="all__name">{{ note.title }}</span>
          <span class="all__meta">第 {{ note.rev }} 版 · {{ note.bytes }} 字节 · 修改于 {{ note.modified }}</span>
        </button>
      </li>
    </ol>
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

.all__count {
  margin: 6px 0 0;
  color: var(--text-dim);
  font-size: 13px;
}

.all__section {
  margin: 26px 0 0;
  padding-bottom: 6px;
  border-bottom: 1px solid var(--border);
  color: var(--text-dim);
  font-size: 14px;
  font-weight: 500;
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
