<script setup lang="ts">
import { Download, History, Pencil, Star, Trash2, type LucideIcon } from "@lucide/vue";
import { computed } from "vue";
import type { Via } from "../../bindings/address.ts";
import ViaHint from "./ViaHint.vue";

/**
 * 页面标题栏：**这一页的主标题**与它的动作。
 *
 * 与窗口顶上的标题栏（`WindowTitleBar`，那一条是地址栏）分工不同：这里写的是
 * **文档自己的名字**，认的是仓库里的标题；那边写的是地址，认的是"现在在哪"。
 *
 * 它贴着阅读区顶部粘住：正文滚下去以后收成一条细栏（标题缩一号、只留图标），
 * 于是"我在看哪一篇、能做什么"始终在视野里。背景必须不透明 —— 否则字会从底下透上来。
 */
const props = defineProps<{
  title: string;
  /** 子页面（标题里有斜杠）的上一级；空串表示没有上一级 */
  parent: string;
  /** 正文滚下去了：收起并粘顶 */
  collapsed: boolean;
  /** 动作按钮；不给就用标准那三个（编辑 / 版本历史 / 删除） */
  actions?: PageAction[];
  /** 被指令带过来时的"从哪儿来"（`$$COMMAND$$` 那一页） */
  via?: Via | null;
  /** 这一页星标过没有 */
  starred?: boolean;
}>();

const emit = defineEmits<{
  (e: "action", name: string): void;
  /** 点了「返回上一级」 */
  (e: "open-parent", title: string): void;
  /** 点了"从哪儿来"那一行里的来源 */
  (e: "open-via", input: string): void;
  /** 加/去星标；地址与标题由持有当前页的一方给 */
  (e: "toggle-star"): void;
}>();

export interface PageAction {
  name: string;
  label: string;
  icon: LucideIcon;
  /** 破坏性的动作用危险色，免得一串按钮里看不出哪个是 */
  danger?: boolean;
}

/** 默认几个：读一篇笔记时真用得上的那几件事 */
const STANDARD: PageAction[] = [
  { name: "edit", label: "编辑", icon: Pencil },
  { name: "history", label: "版本历史", icon: History },
  { name: "export", label: "导出", icon: Download },
  { name: "delete", label: "删除", icon: Trash2, danger: true },
];

const shown = computed(() => props.actions ?? STANDARD);
</script>

<template>
  <div class="page-header" :class="{ 'page-header--collapsed': collapsed }">
    <div class="page-heading">
      <h1 class="page-title selectable" :title="title">{{ title }}</h1>

      <!-- 被指令带过来的：说清"你点的不是这一页" -->
      <ViaHint :via="via ?? null" @open="emit('open-via', $event)"/>

      <!-- 子页面给一个回上一级的出口：斜杠就是父子关系 -->
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
      <!-- 星标：一枚图标按钮，加没加过一眼看得出 -->
      <button
          class="page-action page-action--star"
          :class="{ 'page-action--starred': starred }"
          type="button"
          :title="starred ? '取消星标' : '加星标（会出现在新标签页上）'"
          :aria-label="starred ? '取消星标' : '加星标'"
          :aria-pressed="starred ? 'true' : 'false'"
          @click="emit('toggle-star')"
      >
        <Star :size="16" :stroke-width="1.75" :fill="starred ? 'currentColor' : 'none'"/>
      </button>

      <button
          v-for="action in shown"
          :key="action.name"
          class="page-action"
          :class="{ 'page-action--danger': action.danger }"
          type="button"
          :title="action.label"
          :aria-label="action.label"
          @click="emit('action', action.name)"
      >
        <component :is="action.icon" :size="16" :stroke-width="1.75"/>
        <span class="page-action__label">{{ action.label }}</span>
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
  transition:
      padding 160ms ease,
      border-color 160ms ease;
}

.page-header--collapsed {
  padding: 8px 0;
  border-bottom-color: var(--border);
}

.page-heading {
  flex: 1 1 auto;
  min-width: 0;
}

/* 子页面回上一级：一行小字，压在标题底下 */
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

/* 刻意比正文里的 H1（1.55em）大出一档：页面标题是这一页的主标题，
   正文的 H1 不该压过它 */
.page-title {
  margin: 0;
  font-size: 2.15em;
  font-weight: 600;
  /* 1.5 的行高同时服务两种状态：展开时给多行标题留出舒展的行距；
     收起成粘顶细栏时那条栏仍然截断，行盒不够高会把 CJK 字的底部裁掉 */
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
  transition:
      background-color 120ms ease,
      color 120ms ease;
}

.page-action:hover {
  background: var(--hover);
  color: var(--text);
}

.page-action:active {
  background: var(--press);
}

/* 星标：加过的那一枚用主题色、实心，没加过是空心 */
.page-action--starred {
  color: var(--accent-soft);
}

.page-action--star {
  padding: 6px 8px;
}

/* 只有破坏性的那个用危险色，免得一串按钮里看不出哪个是 */
.page-action--danger:hover {
  background: transparent;
  color: var(--danger);
}

/* 收起后只留图标，并收窄内边距 */
.page-header--collapsed .page-action {
  padding: 6px 8px;
}

.page-header--collapsed .page-action__label {
  display: none;
}

@media (prefers-reduced-motion: reduce) {
  .page-header,
  .page-title {
    transition: none;
  }
}
</style>
