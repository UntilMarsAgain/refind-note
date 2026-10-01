<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { BookOpen } from "@lucide/vue";
import type { HelpPage } from "../../bindings/help.ts";
import { browsingHistory } from "../../core/browsing.ts";
import { metaOf } from "../../core/special.ts";

/**
 * 新标签页（`special:newtab`）。
 *
 * 它是"从哪儿开始"的那一页：一个输入框（打开或新建），加上几行去处。
 * 摆在前面的只有两样 —— **帮助**（新来的人先看它）与**常用页面**；
 * 其余的收在后面一行，不抢眼。
 */
const emit = defineEmits<{
  /** 打开某个地址（笔记名或 `Help:…` / `special:…`） */
  (e: "open", value: string): void;
}>();

const typed = ref("");
/** 后端说了有哪些特殊页 —— 这一页只负责显示 */
const pages = ref<string[]>([]);
/** 帮助页：随程序发布 */
const help = ref<HelpPage[]>([]);

onMounted(async () => {
  try {
    pages.value = await invoke<string[]>("special_pages");
  } catch (error) {
    console.warn("取特殊页面清单失败：", error);
    pages.value = [];
  }

  try {
    help.value = await invoke<HelpPage[]>("help_pages");
  } catch (error) {
    console.warn("取帮助页清单失败：", error);
    help.value = [];
  }
});

/** 摆在第一行的几个：日常最常用 */
const COMMON = ["all", "changes", "history", "files"];

const common = computed(() =>
  COMMON.filter((page) => pages.value.includes(page)).map((page) => ({
    page,
    ...metaOf(page),
  })),
);

/** 其余的：维护与设置 */
const upkeep = computed(() =>
  pages.value
    .filter((page) => page !== "newtab" && !COMMON.includes(page))
    .map((page) => ({ page, ...metaOf(page) })),
);

/** 最近打开：浏览历史的头几条（本身就是按时间倒序、按地址去重的） */
const recent = computed(() => browsingHistory.value.slice(0, 5));

function submit() {
  const value = typed.value.trim();
  if (value) {
    emit("open", value);
  }
}
</script>

<template>
  <section class="newtab">
    <h1 class="newtab__title">新标签页</h1>

    <div class="newtab__field">
      <input
          v-model="typed"
          class="newtab__input"
          type="text"
          placeholder="输入笔记名称即可打开或创建"
          @keydown.enter.prevent="submit"
      />
      <button class="newtab__go" type="button" @click="submit">打开</button>
    </div>

    <!-- 帮助摆在最前面：新来的人先看它，比什么都管用 -->
    <p v-if="help.length > 0" class="newtab__line newtab__line--help">
      <BookOpen :size="14" :stroke-width="1.9"/>
      <span class="newtab__label">帮助</span>
      <button
          v-for="page in help"
          :key="page.slug"
          type="button"
          class="newtab__link newtab__link--help"
          :title="page.display"
          @click="emit('open', `Help:${page.slug}`)"
      >
        {{ page.title }}
      </button>
    </p>

    <p class="newtab__line">
      <span class="newtab__label">常用</span>
      <button
          v-for="item in common"
          :key="item.page"
          type="button"
          class="newtab__link"
          :title="item.tip"
          @click="emit('open', `special:${item.page}`)"
      >
        {{ item.label }}
      </button>
    </p>

    <p v-if="upkeep.length > 0" class="newtab__line newtab__line--dim">
      <span class="newtab__label">更多</span>
      <button
          v-for="item in upkeep"
          :key="item.page"
          type="button"
          class="newtab__link"
          :title="item.tip"
          @click="emit('open', `special:${item.page}`)"
      >
        {{ item.label }}
      </button>
    </p>

    <p v-if="recent.length > 0" class="newtab__line newtab__line--dim">
      <span class="newtab__label">最近</span>
      <button
          v-for="visit in recent"
          :key="visit.address"
          type="button"
          class="newtab__link"
          :title="visit.address"
          @click="emit('open', visit.address)"
      >
        {{ visit.title || visit.address }}
      </button>
    </p>
  </section>
</template>

<style scoped>
.newtab {
  max-width: 560px;
  margin: 52px auto 0;
}

.newtab__title {
  margin: 0 0 14px;
  font-size: 20px;
  font-weight: 600;
  text-align: center;
}

.newtab__field {
  display: flex;
  gap: 8px;
}

.newtab__input {
  flex: 1;
  min-width: 0;
  height: 34px;
  padding: 0 12px;
  border: 1px solid var(--border);
  border-radius: 7px;
  background: var(--field-bg);
  color: var(--text);
  font: inherit;
  font-size: 13.5px;
}

.newtab__input:focus {
  outline: none;
  border-color: var(--accent-soft);
}

.newtab__input::placeholder {
  color: var(--text-dim);
}

.newtab__go {
  height: 34px;
  padding: 0 16px;
  border: 1px solid var(--accent-soft);
  border-radius: 7px;
  background: transparent;
  color: var(--accent-soft);
  font: inherit;
  font-size: 13.5px;
  cursor: pointer;
}

.newtab__go:hover {
  background: var(--accent);
  color: var(--text);
}

/* 一行去处：标签 + 一串链接，不加框、不加底色 */
.newtab__line {
  display: flex;
  flex-wrap: wrap;
  gap: 6px 12px;
  align-items: baseline;
  margin: 22px 0 0;
  font-size: 13px;
  line-height: 1.9;
}

.newtab__line--help {
  align-items: center;
  margin-top: 18px;
  color: var(--accent-soft);
}

.newtab__line--dim {
  font-size: 12.5px;
}

.newtab__label {
  flex: 0 0 auto;
  min-width: 28px;
  color: var(--text-dim);
  font-size: 12px;
}

.newtab__link {
  padding: 0;
  border: 0;
  background: transparent;
  color: var(--text-dim);
  font: inherit;
  font-size: inherit;
  cursor: pointer;
}

.newtab__link:hover {
  color: var(--text);
  text-decoration: underline;
}

/* 帮助那一行用主题色：它是这一页唯一"希望你点"的地方 */
.newtab__link--help {
  color: var(--accent-soft);
}
</style>
