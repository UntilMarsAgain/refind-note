<script setup lang="ts">
import { ref, type Component } from "vue";
import {
  Bookmark,
  Clock,
  FileText,
  Link2,
  PanelLeftClose,
  PanelLeftOpen,
  Plus,
  Settings,
  ShieldCheck,
  Star,
} from "@lucide/vue";

/**
 * 垂直标签栏，造型对齐参考（Firefox 垂直标签页）。
 *
 * ⚠️ 全是占位数据：真正的页面切换要等后端能按标题取笔记。
 *
 * 三条约定：
 * 1. 最顶部那一行是展开 / 收起按钮，展开时同时显示名称；
 * 2. 收起时只显示图标；展开后**每一行都带名称**（标签页、固定项、底部工具都一样）；
 * 3. 说明气泡只在收起时出现——展开后名称已可见，气泡就是重复的。
 */

interface RailItem {
  id: string;
  title: string;
  icon: Component;
}

/** 固定项：放在分隔线之上的快捷入口 */
const pinned: RailItem[] = [
  { id: "pinned-shield", title: "隐私与安全", icon: ShieldCheck },
  { id: "pinned-bookmark", title: "书签", icon: Bookmark },
  { id: "pinned-link", title: "外部链接", icon: Link2 },
];

const tabs: RailItem[] = [
  { id: "tab-1", title: "渲染链路验证", icon: FileText },
  { id: "tab-2", title: "平陆运河", icon: FileText },
  { id: "tab-3", title: "笔记语法速览", icon: FileText },
];

const footer: RailItem[] = [
  { id: "foot-history", title: "历史", icon: Clock },
  { id: "foot-star", title: "收藏", icon: Star },
  { id: "foot-settings", title: "设置", icon: Settings },
];

/** 默认收起，只显示图标 */
const collapsed = ref(true);
const activeTab = ref("tab-1");

function openTab(id: string) {
  // TODO: 后端支持按标题取笔记后，换成真正的页面切换
  activeTab.value = id;
  console.debug("open tab:", id);
}
</script>

<template>
  <aside class="rail" :class="{ 'rail--collapsed': collapsed }">
    <!-- 最顶部：展开 / 收起，展开时一并显示名称 -->
    <button
      class="rail__button rail__toggle"
      type="button"
      :data-tip="collapsed ? '展开标签栏' : null"
      :aria-label="collapsed ? '展开标签栏' : '收起标签栏'"
      :aria-expanded="!collapsed"
      @click="collapsed = !collapsed"
    >
      <component
        :is="collapsed ? PanelLeftOpen : PanelLeftClose"
        class="rail__icon"
        :size="16"
        :stroke-width="1.9"
      />
      <span class="rail__label">标签页</span>
    </button>

    <div class="rail__group">
      <button
        v-for="item in pinned"
        :key="item.id"
        class="rail__button"
        type="button"
        :data-tip="collapsed ? item.title : null"
        :aria-label="item.title"
      >
        <component
          :is="item.icon"
          class="rail__icon"
          :size="16"
          :stroke-width="1.9"
        />
        <span class="rail__label">{{ item.title }}</span>
      </button>
    </div>

    <div class="rail__tabs">
      <button
        v-for="item in tabs"
        :key="item.id"
        class="rail__button"
        :class="{ 'rail__button--active': activeTab === item.id }"
        type="button"
        :data-tip="collapsed ? item.title : null"
        :aria-label="item.title"
        :aria-current="activeTab === item.id ? 'page' : undefined"
        @click="openTab(item.id)"
      >
        <component
          :is="item.icon"
          class="rail__icon"
          :size="16"
          :stroke-width="1.9"
        />
        <span class="rail__label">{{ item.title }}</span>
      </button>

      <button
        class="rail__button"
        type="button"
        :data-tip="collapsed ? '新建标签页' : null"
        aria-label="新建标签页"
      >
        <Plus class="rail__icon" :size="16" :stroke-width="1.9" />
        <span class="rail__label">新建标签页</span>
      </button>
    </div>

    <div class="rail__group rail__group--footer">
      <button
        v-for="item in footer"
        :key="item.id"
        class="rail__button"
        type="button"
        :data-tip="collapsed ? item.title : null"
        :aria-label="item.title"
      >
        <component
          :is="item.icon"
          class="rail__icon"
          :size="16"
          :stroke-width="1.9"
        />
        <span class="rail__label">{{ item.title }}</span>
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

.rail__group,
.rail__tabs {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.rail__toggle {
  margin-bottom: 6px;
}

.rail__tabs {
  margin-top: 6px;
  padding-top: 6px;
  border-top: 1px solid var(--border);
}

.rail__group--footer {
  margin-top: auto;
  padding-top: 8px;
  border-top: 1px solid var(--border);
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
