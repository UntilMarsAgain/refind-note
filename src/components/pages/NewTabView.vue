<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { browsingHistory } from "../../core/browsing.ts";
import { metaOf } from "../../core/special.ts";

/**
 * 新标签页（`special:newtab`）。
 *
 * 它是"从哪儿开始"的那一页：一个输入框（打开或新建），加上**常用的几个去处** ——
 * 冷启动时最难的从来不是"怎么用"，而是"有什么可以去"。所以这里把入口摆出来，
 * 再把最近看过的几页递过去。
 */
const emit = defineEmits<{
  /** 打开某个地址（笔记名或 `special:…`） */
  (e: "open", value: string): void;
}>();

const typed = ref("");
/** 后端说了有哪些特殊页 —— 这一页只负责显示 */
const pages = ref<string[]>([]);

onMounted(async () => {
  try {
    pages.value = await invoke<string[]>("special_pages");
  } catch (error) {
    console.warn("取特殊页面清单失败：", error);
    pages.value = [];
  }
});

/** 新标签页自己不用列出来 */
const entries = computed(() => pages.value.filter((page) => page !== "newtab"));

/** 摆在前面的几个：日常最常用 */
const COMMON = ["all", "changes", "history", "files", "random"];
/** 其余的收在"维护"一组 */
const UPKEEP = ["trash", "gc", "keys", "settings", "debug"];

function ordered(wanted: string[]): string[] {
  return wanted.filter((page) => entries.value.includes(page));
}

const common = computed(() => ordered(COMMON));
const upkeep = computed(() => ordered(UPKEEP));
/** 后端新加了页面而这里还没归类：也要露出来，不能藏 */
const rest = computed(() =>
  entries.value.filter((page) => !COMMON.includes(page) && !UPKEEP.includes(page)),
);

/** 最近打开：浏览历史的头几条（它本身就是按时间倒序、按地址去重的） */
const recent = computed(() => browsingHistory.value.slice(0, 6));

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

    <p class="newtab__hint">
      输入笔记名称即可打开，名称不存在时可直接创建；以
      <code>special:</code> 开头的标识用于打开系统页面。也可直接使用地址栏。
    </p>

    <div class="newtab__field">
      <input
          v-model="typed"
          class="newtab__input"
          type="text"
          placeholder="笔记名称，或 命名空间:笔记名称"
          @keydown.enter.prevent="submit"
      />
      <button class="newtab__go" type="button" @click="submit">打开</button>
    </div>

    <div class="newtab__group">
      <h2 class="newtab__caption">常用</h2>
      <div class="newtab__links">
        <button
            v-for="page in common"
            :key="page"
            type="button"
            class="newtab__link"
            :title="metaOf(page).tip"
            @click="emit('open', `special:${page}`)"
        >
          <component v-if="metaOf(page).icon" :is="metaOf(page).icon" :size="15" :stroke-width="1.8"/>
          {{ metaOf(page).label }}
        </button>
      </div>
    </div>

    <div v-if="upkeep.length > 0 || rest.length > 0" class="newtab__group">
      <h2 class="newtab__caption">维护与设置</h2>
      <div class="newtab__links">
        <button
            v-for="page in [...upkeep, ...rest]"
            :key="page"
            type="button"
            class="newtab__link"
            :title="metaOf(page).tip"
            @click="emit('open', `special:${page}`)"
        >
          <component v-if="metaOf(page).icon" :is="metaOf(page).icon" :size="15" :stroke-width="1.8"/>
          {{ metaOf(page).label }}
        </button>
      </div>
    </div>

    <div v-if="recent.length > 0" class="newtab__group">
      <h2 class="newtab__caption">最近打开</h2>
      <div class="newtab__recents">
        <button
            v-for="visit in recent"
            :key="visit.address"
            type="button"
            class="newtab__recent"
            :title="visit.address"
            @click="emit('open', visit.address)"
        >
          <span class="newtab__recent-name">{{ visit.title || visit.address }}</span>
          <span class="newtab__recent-address">{{ visit.address }}</span>
        </button>
      </div>
    </div>
  </section>
</template>

<style scoped>
.newtab {
  max-width: 640px;
  margin: 44px auto 0;
}

.newtab__title {
  margin: 0 0 10px;
  font-size: 22px;
  font-weight: 600;
  text-align: center;
}

.newtab__hint {
  margin: 0 0 18px;
  color: var(--text-dim);
  font-size: 13px;
  line-height: 1.8;
  text-align: center;
}

.newtab__hint code {
  padding: 1px 5px;
  border-radius: 4px;
  background: var(--hover);
  font-family: var(--mono-font);
  font-size: 0.92em;
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

.newtab__group {
  margin-top: 26px;
}

.newtab__caption {
  margin: 0 0 8px;
  color: var(--text-dim);
  font-size: 12px;
  font-weight: 600;
  letter-spacing: 0.03em;
}

.newtab__links {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.newtab__link {
  display: inline-flex;
  gap: 6px;
  align-items: center;
  padding: 6px 12px;
  border: 1px solid var(--border);
  border-radius: 999px;
  background: transparent;
  color: var(--text-dim);
  font: inherit;
  font-size: 13px;
  cursor: pointer;
}

.newtab__link:hover {
  border-color: var(--accent-soft);
  background: var(--accent-tint);
  color: var(--text);
}

/* 最近打开：一列细行，与浏览历史那一页同一个读法 */
.newtab__recents {
  display: flex;
  flex-direction: column;
  border-top: 1px solid var(--border);
}

.newtab__recent {
  display: grid;
  grid-template-columns: minmax(0, 12em) minmax(0, 1fr);
  gap: 12px;
  align-items: baseline;
  padding: 7px 8px;
  border: 0;
  border-bottom: 1px solid var(--border);
  background: transparent;
  color: var(--text);
  font: inherit;
  font-size: 13px;
  text-align: left;
  cursor: pointer;
}

.newtab__recent:hover {
  background: var(--accent-tint);
}

.newtab__recent-name {
  font-weight: 600;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.newtab__recent-address {
  color: var(--text-dim);
  font-size: 12.5px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
</style>
