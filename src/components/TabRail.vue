<script setup lang="ts">
import { ref } from "vue";
import { FileText, PanelLeftClose, PanelLeftOpen, Plus } from "@lucide/vue";
import { PREFERENCE_KEYS, readFlag, writeFlag } from "../settings";

/**
 * 垂直标签栏，造型对齐参考（Firefox 垂直标签页）。
 *
 * 每一行都对应仓库里**真实的笔记**（`title` 就是打开它需要的参数），没有一个占位项。
 *
 * 三条约定：
 * 1. 最顶部那一行是展开 / 收起按钮，展开时同时显示名称；
 * 2. 收起时只显示图标；展开后每一行都带名称；
 * 3. 说明气泡只在收起时出现——展开后名称已可见，气泡就是重复的。
 *
 * ⚠️ `.rail` 与笔记列表都刻意**不加 overflow**，否则向右弹出的气泡会被裁掉
 * （`overflow-x: visible` 与 `overflow-y: auto` 不能共存）。所以收起态下笔记多到
 * 一栏放不下时会溢出。正解是把这条栏做成真正的「打开的标签页」列表（数量本就少）。
 */

/** 与 Rust 端 `NoteSummary` 对应（这里只用到 key 与 title） */
interface NoteTab {
  key: string;
  title: string;
}

defineProps<{
  /** 仓库里的笔记，已按标题排序 */
  notes: NoteTab[];
  /** 当前打开笔记的规范键 */
  active: string;
}>();

const emit = defineEmits<{
  (e: "open", title: string): void;
  (e: "create"): void;
}>();

/** 默认收起；展开状态记进界面偏好，下次打开保持原样 */
const collapsed = ref(readFlag(PREFERENCE_KEYS.railCollapsed, true));

function toggle() {
  collapsed.value = !collapsed.value;
  writeFlag(PREFERENCE_KEYS.railCollapsed, collapsed.value);
}
</script>

<template>
  <aside class="rail" :class="{ 'rail--collapsed': collapsed }">
    <!-- 最顶部：展开 / 收起，展开时一并显示名称 -->
    <button
      class="rail__button rail__toggle"
      type="button"
      :data-tip="collapsed ? '展开笔记栏' : null"
      :aria-label="collapsed ? '展开笔记栏' : '收起笔记栏'"
      :aria-expanded="!collapsed"
      @click="toggle"
    >
      <component
        :is="collapsed ? PanelLeftOpen : PanelLeftClose"
        class="rail__icon"
        :size="16"
        :stroke-width="1.9"
      />
      <span class="rail__label">笔记</span>
    </button>

    <div class="rail__tabs">
      <button
        v-for="item in notes"
        :key="item.key"
        class="rail__button"
        :class="{ 'rail__button--active': active === item.key }"
        type="button"
        :data-tip="collapsed ? item.title : null"
        :aria-label="item.title"
        :aria-current="active === item.key ? 'page' : undefined"
        @click="emit('open', item.title)"
      >
        <FileText class="rail__icon" :size="16" :stroke-width="1.9" />
        <span class="rail__label">{{ item.title }}</span>
      </button>

      <button
        class="rail__button"
        type="button"
        :data-tip="collapsed ? '新建笔记' : null"
        aria-label="新建笔记"
        @click="emit('create')"
      >
        <Plus class="rail__icon" :size="16" :stroke-width="1.9" />
        <span class="rail__label">新建笔记</span>
      </button>
    </div>
  </aside>
</template>

<style scoped>
.rail {
  display: flex;
  flex: 0 0 auto;
  flex-direction: column;
  gap: 2px;
  width: 200px;
  padding: 8px 6px;
  border-right: 1px solid var(--border);
  background: var(--bg);
  /* 刻意不加 overflow：会把向右弹出的说明气泡裁掉。
     等真标签页多到需要滚动时，气泡得改用 portal 渲染。 */
  transition: width 200ms ease;
}

.rail--collapsed {
  width: 48px;
}

.rail__toggle {
  margin-bottom: 6px;
}

.rail__tabs {
  display: flex;
  flex-direction: column;
  gap: 2px;
  margin-top: 6px;
  padding-top: 6px;
  border-top: 1px solid var(--border);
}

/* 展开时才允许滚动：此时名称已经可见、不需要气泡，滚动不会裁到东西。
   收起态刻意不滚——滚了就会把向右弹出的气泡裁掉。 */
.rail:not(.rail--collapsed) .rail__tabs {
  flex: 1 1 auto;
  min-height: 0;
  overflow-y: auto;
}

.rail__button {
  appearance: none;
  display: flex;
  align-items: center;
  gap: 10px;
  width: 100%;
  height: 34px;
  /* 收起时按钮宽 36px，图标正好落在中间（10 + 8 = 18 = 36 / 2），
     所以展开 / 收起切换时图标不会横跳 */
  padding: 0 10px;
  border: 0;
  border-radius: 6px;
  background: transparent;
  color: var(--text-dim);
  font-size: 13px;
  /* 不能是 1：CJK 字体的 ascent + descent 约 1.48em，行盒只有 1em 时，
     底部笔画的墨迹会落到行盒外，再被 .rail__label 的 overflow:hidden 裁掉 */
  line-height: 1.6;
  white-space: nowrap;
  text-align: left;
  cursor: pointer;
  transition: background-color 120ms ease, color 120ms ease;
}

.rail__button:hover {
  background: var(--hover);
  color: var(--text);
}

.rail__button:active {
  background: var(--press);
}

.rail__button--active {
  /* 参考里当前标签是带框的；用 inset 阴影，免掉切页时的布局跳动 */
  background: var(--hover);
  box-shadow: inset 0 0 0 1px var(--border);
  color: var(--text);
}

.rail__icon {
  flex: 0 0 auto;
}

.rail__label {
  flex: 1 1 auto;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* 收起后只留图标 */
.rail--collapsed .rail__label {
  display: none;
}

@media (prefers-reduced-motion: reduce) {
  .rail {
    transition: none;
  }
}
</style>
