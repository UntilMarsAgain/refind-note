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

<script setup lang="ts">
import { ref } from "vue";
import { ListTree } from "@lucide/vue";
import type { OutlineEntry } from "../../core/outline.ts";

/**
 * 本页目录。
 *
 * ## 为什么摆在正文**上面**、默认收起
 *
 * 摆在右边那类"常驻侧栏"看着时髦，但这里不合适：阅读宽度是限的（`--reading-width`），
 * 侧栏要么把正文挤窄，要么压在正文上；而正文上方那一块本来就空着（标题与元信息之间）。
 * 收起是默认：目录是**导航**，不是内容 —— 没人想先读一遍目录再读正文。
 *
 * ## 为什么不自己滚动
 *
 * 滚动的活交给浏览器（`scrollIntoView`），与正文里点锚点是同一条路 ——
 * 那样"点了标题"与"点了正文里的 `[文字](#标题)`"表现完全一致，
 * 包括地址栏跟着变（`section` 事件往上报，与 `NoteContent` 里那条一致）。
 */
const props = defineProps<{
    /** 目录项（层级已经算好，见 `core/outline.ts`） */
    entries: OutlineEntry[];
    /** 当前所在的那一节（点过之后标出来） */
    active: string;
}>();

const emit = defineEmits<{
    /** 点了某一条：滚到那一节，并把章节报给上层叠进地址 */
    (e: "pick", id: string): void;
}>();

const open = ref(false);

/** 标题栏那一行的文案：几节、展开还是收起 */
const summary = () => (open.value ? "收起本页目录" : `本页目录（${props.entries.length} 节）`);

function pick(id: string) {
    emit("pick", id);
}
</script>

<template>
  <!--
    `v-if` 在外：条目不足时连边框都不画。
    收起时内容仍在 DOM 里（只是不显示）—— `display: none` 的目录不占地方，
    展开时也不用重新算一次。
  -->
  <div v-if="entries.length" class="outline" :class="{ 'outline--open': open }">
    <button
        class="outline__toggle"
        type="button"
        :aria-expanded="open"
        @click="open = !open"
    >
      <ListTree :size="14" :stroke-width="1.9"/>
      <span>{{ summary() }}</span>
    </button>

    <ol v-show="open" class="outline__list">
      <li
          v-for="entry in entries"
          :key="entry.id"
          class="outline__item"
          :style="{ '--depth': entry.depth }"
      >
        <button
            class="outline__link"
            :class="{ 'outline__link--active': entry.id === active }"
            type="button"
            @click="pick(entry.id)"
        >
          {{ entry.text }}
        </button>
      </li>
    </ol>
  </div>
</template>

<style scoped>
/*
  配色全走 CSS 变量：深浅两套都在 `styles/theme.css` 里定过，
  这里只管排版 —— 换主题时这一块不用动。
*/
.outline {
  margin: 10px 0 14px;
  border: 1px solid var(--border);
  border-radius: 8px;
  background: var(--surface);
}

.outline__toggle {
  display: flex;
  gap: 7px;
  align-items: center;
  width: 100%;
  padding: 7px 11px;
  border: 0;
  border-radius: 8px;
  background: transparent;
  color: var(--text-dim);
  font: inherit;
  font-size: 12.5px;
  text-align: left;
  cursor: pointer;
}

.outline__toggle:hover {
  color: var(--text);
  background: var(--hover);
}

.outline--open .outline__toggle {
  border-bottom: 1px solid var(--divider);
  border-radius: 8px 8px 0 0;
}

.outline__list {
  margin: 0;
  padding: 5px 0 7px;
  /* 目录自己的序号没有意义（那会变成 1. 1.1. 1.1.1. 三层数字） */
  list-style: none;
  /* 上限：长笔记的目录能有上百条，全铺开会把正文顶到屏幕外 */
  max-height: 40vh;
  overflow-y: auto;
}

.outline__item {
  margin: 0;
}

/* 缩进按"这一篇实际用到的那几级"算，不按 h1…h6 —— 见 core/outline.ts */
.outline__link {
  display: block;
  width: 100%;
  padding: 3px 11px 3px calc(11px + var(--depth, 0) * 16px);
  border: 0;
  background: transparent;
  color: var(--text-dim);
  font: inherit;
  font-size: 13px;
  line-height: 1.55;
  text-align: left;
  cursor: pointer;
}

.outline__link:hover {
  background: var(--hover);
  color: var(--text);
}

.outline__link--active {
  color: var(--accent-soft);
  font-weight: 600;
}
</style>