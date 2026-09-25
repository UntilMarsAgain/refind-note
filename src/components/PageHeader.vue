<script setup lang="ts">
import type { Component } from "vue";
import {
  Brackets,
  Ellipsis,
  History,
  Languages,
  MessageSquare,
  Pencil,
  Share2,
} from "@lucide/vue";

/**
 * 页面标题行 + 页面操作。
 *
 * 贴顶冻结用 `position: sticky` 实现；滚下去之后收起（标题变小、操作只剩图标），
 * 收起状态由父组件通过 `collapsed` 传入。
 */

defineProps<{
  title: string;
  /** 正文已滚下去：收起并冻结 */
  collapsed: boolean;
}>();

const emit = defineEmits<{ (e: "action", name: string): void }>();

/** 先摆一排假按钮占位，功能之后再接 */
const actions: {
  name: string;
  label: string;
  icon: Component;
  iconOnly?: boolean;
}[] = [
  { name: "share", label: "分享", icon: Share2, iconOnly: true },
  { name: "language", label: "语言", icon: Languages },
  { name: "history", label: "查看历史", icon: History },
  { name: "edit", label: "编辑", icon: Pencil },
  { name: "source", label: "源代码", icon: Brackets },
  { name: "discuss", label: "讨论", icon: MessageSquare },
  { name: "more", label: "更多", icon: Ellipsis, iconOnly: true },
];
</script>

<template>
  <div class="page-header" :class="{ 'page-header--collapsed': collapsed }">
    <h1 class="page-title" :title="title">{{ title }}</h1>

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
.page-title {
  flex: 1 1 auto;
  min-width: 0;
  margin: 0;
  font-size: 2.15em;
  font-weight: 600;
  line-height: 1.3;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  transition: font-size 160ms ease;
}

.page-header--collapsed .page-title {
  font-size: 1.05em;
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
  line-height: 1;
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
