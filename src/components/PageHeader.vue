<script setup lang="ts">
import type { Component } from "vue";
import { History, Pencil, Trash2 } from "@lucide/vue";

/**
 * 页面标题行 + 页面操作。
 *
 * 贴顶冻结用 `position: sticky` 实现；滚下去之后收起（标题变小、操作只剩图标），
 * 收起状态由父组件通过 `collapsed` 传入。
 *
 * 操作只留**真能用的**：这里曾经摆过一排假按钮（分享 / 语言 / 源代码 / 讨论 /
 * 更多），现在保留的每一个都接得到真实功能——编辑会打开编辑器，版本历史会打开
 * 历史与对比页。等某个功能做出来了再加回对应按钮，不再用点不动的按钮占位。
 */

defineProps<{
  title: string;
  /** 有斜杠（子页面）时上一级的标题；空串表示没有 */
  parent: string;
  /** 正文已滚下去：收起并冻结 */
  collapsed: boolean;
}>();

const emit = defineEmits<{
  (e: "action", name: string): void;
  (e: "open-parent", title: string): void;
}>();

const actions: {
  name: string;
  label: string;
  icon: Component;
  iconOnly?: boolean;
}[] = [
  { name: "edit", label: "编辑", icon: Pencil },
  { name: "history", label: "版本历史", icon: History },
  { name: "delete", label: "删除", icon: Trash2 },
];
</script>

<template>
  <div class="page-header" :class="{ 'page-header--collapsed': collapsed }">
    <div class="page-heading">
      <h1 class="page-title" :title="title">{{ title }}</h1>
      <!-- 子页面（标题里有斜杠）给一个回上一级的出口 -->
      <button
        v-if="parent"
        class="page-up"
        type="button"
        @click="emit('open-parent', parent)"
      >
        ◀ 返回上一级：{{ parent }}
      </button>
    </div>

    <div class="page-actions">
      <button
        v-for="action in actions"
        :key="action.name"
        class="page-action"
        type="button"
        :title="action.label"
        :aria-label="action.label"
        @click="emit('action', action.name)"
      >
        <component :is="action.icon" :size="16" :stroke-width="1.75" />
        <span v-if="!action.iconOnly" class="page-action__label">
          {{ action.label }}
        </span>
      </button>
    </div>
  </div>
</template>

<style scoped>
.page-header {
  position: sticky;
  top: 0;
  z-index: 10;
  display: flex;
  align-items: center;
  gap: 20px;
  padding: 22px 0 14px;
  /* 必须不透明，否则正文会从标题底下透出来 */
  background: var(--bg);
  border-bottom: 1px solid transparent;
  transition: padding 160ms ease, border-color 160ms ease;
}

.page-header--collapsed {
  padding: 8px 0;
  border-bottom-color: var(--border);
}

/* 刻意比正文里的 H1（1.55em）大出一档：页面标题是这一页的主标题，
   正文的 H1 不该压过它 */
.page-heading {
  flex: 1 1 auto;
  min-width: 0;
}

.page-up {
  appearance: none;
  display: block;
  margin-top: 2px;
  padding: 0;
  border: 0;
  background: transparent;
  color: var(--text-dim);
  font-size: 12.5px;
  line-height: 1.5;
  cursor: pointer;
}

.page-up:hover {
  color: var(--accent-soft);
}

.page-title {
  flex: 1 1 auto;
  min-width: 0;
  margin: 0;
  font-size: 2.15em;
  font-weight: 600;
  /* 1.5 的行高同时服务两种状态：展开时给多行标题留出舒展的行距；收起成粘顶细栏时
     那条栏仍然截断（见下），行盒不够高会把 CJK 字的底部裁掉 */
  line-height: 1.5;
  /* 标题过长**换行**，不截断成省略号 —— 标题是这一页的身份，看不全很要命 */
  white-space: normal;
  /* 长串英文/链接没有空格可断，不这样会顶破容器 */
  overflow-wrap: anywhere;
  transition: font-size 160ms ease;
}

.page-header--collapsed .page-title {
  font-size: 1.05em;
  /* 收起后是粘在顶部的一条细栏，必须只有一行高：这里继续截断 */
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.page-actions {
  display: flex;
  flex: 0 0 auto;
  align-items: center;
  gap: 2px;
}

.page-action {
  appearance: none;
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 6px 10px;
  border: 0;
  border-radius: 6px;
  background: transparent;
  color: var(--text-dim);
  font-size: 13.5px;
  /* 同理：1 会让 CJK 的墨迹溢出行盒，图标与文字看着就不在一条中线上 */
  line-height: 1.4;
  white-space: nowrap;
  cursor: default;
  transition: background-color 120ms ease, color 120ms ease;
}

.page-action:hover {
  background: var(--hover);
  color: var(--text);
}

.page-action:active {
  background: var(--press);
}

/* 收起后只留图标，并收窄内边距 */
.page-header--collapsed .page-action {
  padding: 6px 8px;
}

.page-header--collapsed .page-action__label {
  display: none;
}
</style>
