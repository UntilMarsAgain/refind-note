<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { Star } from "@lucide/vue";
import type { HelpPage } from "../../ipc/help.ts";
import { starred, toggleStar } from "../../core/preferences.ts";

/**
 * 新标签页（`special:newtab`）。
 *
 * 一竖排入口 + 一片星标，够用就好：
 *
 * - 上面那几行去从是**每个仓库都用得上**的（帮助、全部页面、随机、最近改动、浏览历史），
 *   所以竖着排、字大一点，点起来不用瞄准；
 * - 下面的星标是**你自己攒的**：在任意一页的页头点那颗星，它就落到这里。
 *   星标不设上限，多了这一片自己滚。
 */
const emit = defineEmits<{
  /** 打开某个地址（笔记名或 `Help:…` / `special:…`） */
  (e: "open", value: string): void;
}>();

const typed = ref("");
/** 帮助页：随程序发布 */
const help = ref<HelpPage[]>([]);

onMounted(async () => {
  try {
    help.value = await invoke<HelpPage[]>("help_pages");
  } catch (error) {
    console.warn("取帮助页清单失败：", error);
    help.value = [];
  }
});

/** 帮助首页：就叫「首页」那一页（文件名与页面名是同一个，所以这里按名字找） */
const helpHome = computed(() => help.value.find((page) => page.slug === "首页"));

/** 竖排的那几条：固定顺序，缺哪个就不显示哪个 */
const entries = computed(() =>
  [
    helpHome.value
      ? { key: "help", label: "帮助首页", address: `Help:${helpHome.value.slug}` }
      : null,
    { key: "all", label: "全部页面", address: "special:all" },
    { key: "random", label: "随机页面", address: "special:random" },
    { key: "changes", label: "最近更改", address: "special:changes" },
    { key: "history", label: "浏览历史", address: "special:history" },
  ].filter((entry) => entry !== null),
);

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

    <nav class="newtab__entries">
      <button
          v-for="entry in entries"
          :key="entry.key"
          type="button"
          class="newtab__entry"
          @click="emit('open', entry.address)"
      >
        {{ entry.label }}
      </button>
    </nav>

    <div v-if="starred.length > 0" class="newtab__stars">
      <h2 class="newtab__caption">星标</h2>
      <!-- 星标不设上限：多了就在这一片里滚 -->
      <ul class="newtab__list">
        <li v-for="star in starred" :key="star.address" class="newtab__star">
          <button
              type="button"
              class="newtab__star-open"
              :title="star.address"
              @click="emit('open', star.address)"
          >
            <span class="newtab__star-name">{{ star.title || star.address }}</span>
            <span class="newtab__star-address">{{ star.address }}</span>
          </button>
          <button
              type="button"
              class="newtab__star-off"
              title="取消星标"
              aria-label="取消星标"
              @click="toggleStar(star.address, star.title)"
          >
            <Star :size="13" :stroke-width="1.8" fill="currentColor"/>
          </button>
        </li>
      </ul>
    </div>
  </section>
</template>

<style scoped>
.newtab {
  max-width: 460px;
  margin: 56px auto 0;
}

.newtab__title {
  margin: 0 0 16px;
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
  height: 36px;
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
  height: 36px;
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

/* 竖排入口：一行一个，字比正文大一点，点起来不用瞄准 */
.newtab__entries {
  display: flex;
  flex-direction: column;
  margin-top: 26px;
}

.newtab__entry {
  padding: 10px 4px;
  border: 0;
  border-bottom: 1px solid var(--border);
  background: transparent;
  color: var(--text);
  font: inherit;
  font-size: 15px;
  text-align: left;
  cursor: pointer;
}

.newtab__entry:first-child {
  border-top: 1px solid var(--border);
}

.newtab__entry:hover {
  padding-left: 10px;
  background: var(--accent-tint);
  color: var(--accent-soft);
  transition: padding-left 120ms ease;
}

.newtab__stars {
  margin-top: 28px;
}

.newtab__caption {
  margin: 0 0 6px;
  color: var(--text-dim);
  font-size: 12px;
  font-weight: 600;
  letter-spacing: 0.03em;
}

/* 星标多了就自己滚：这一片有上限，页面整体不被它撑长 */
.newtab__list {
  max-height: 232px;
  margin: 0;
  padding: 0;
  list-style: none;
  overflow-y: auto;
}

.newtab__star {
  display: flex;
  align-items: center;
  gap: 6px;
  border-bottom: 1px solid var(--border);
}

.newtab__star-open {
  display: grid;
  flex: 1 1 auto;
  grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
  gap: 10px;
  align-items: baseline;
  min-width: 0;
  padding: 7px 4px;
  border: 0;
  background: transparent;
  color: var(--text);
  font: inherit;
  font-size: 13px;
  text-align: left;
  cursor: pointer;
}

.newtab__star-open:hover {
  background: var(--accent-tint);
}

.newtab__star-name {
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
}

.newtab__star-address {
  color: var(--text-dim);
  font-size: 12px;
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
}

.newtab__star-off {
  flex: 0 0 auto;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  padding: 0;
  border: 0;
  border-radius: 5px;
  background: transparent;
  color: var(--accent-soft);
  cursor: pointer;
}

.newtab__star-off:hover {
  background: var(--hover);
  color: var(--danger);
}
</style>
