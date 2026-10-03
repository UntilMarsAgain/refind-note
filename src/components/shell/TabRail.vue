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
import { PanelLeftClose, PanelLeftOpen, Plus, Settings, X } from "@lucide/vue";
import { railCollapsed, toggleRail } from "../../core/preferences.ts";
import type { TabState } from "../../core/tabs.ts";
import { initialOf } from "../../core/title.ts";
import { useTabRail } from "../../composables/useTabRail.ts";

/**
 * 垂直标签栏。
 *
 * 这里只留**装配**：摆哪几个按钮、class 怎么挂。真正在动的是三段交互
 * —— 抖动、拖放换序、右键菜单 —— 它们连同各自的时序搬进了
 * `composables/useTabRail.ts`，样式搬进了同目录的 `rail.css`。
 *
 * 本组件**不持有任何标签页数据**：增删改序都是 `emit` 上去，由 `App.vue` 转给
 * `core/tabs.ts`。所以这里没有、也不该有第二份标签页状态。
 */
const props = defineProps<{
  tabs: TabState[];
  active: number;
  /** 有没有"刚关掉、可以重新打开"的标签页（决定右键菜单里给不给那一项） */
  canReopen: boolean;
  /**
   * 抖动信号：每变一次就让标签动一下。
   *
   * 用在"关掉最后一个标签、于是又新建了一个"这种场合 —— 抖一下告诉用户
   * 关闭生效了、只是又开了一个，而不是点了没反应。
   */
  shakeTick: number;
}>();

const emit = defineEmits<{
  (e: "select", index: number): void;
  (e: "close", index: number): void;
  (e: "close-others", index: number): void;
  /** 重新打开刚关掉的那个（右键菜单里给一项入口，快捷键是 Ctrl+Shift+T） */
  (e: "reopen"): void;
  (e: "new-tab"): void;
  /** 拖放调整顺序 */
  (e: "move", from: number, to: number): void;
  (e: "settings"): void;
}>();

// 收的是取值函数而不是值：状态只有一份（在 props 上），这里不留副本。
const {
  shakeIndex,
  dragging,
  overIndex,
  onDragStart,
  onDragOver,
  onDragEnd,
  onDrop,
  onTabMenu,
} = useTabRail({
  active: () => props.active,
  canReopen: () => props.canReopen,
  shakeTick: () => props.shakeTick,
  actions: {
    close: (index) => emit("close", index),
    closeOthers: (index) => emit("close-others", index),
    reopen: () => emit("reopen"),
    move: (from, to) => emit("move", from, to),
  },
});
</script>

<template>
  <aside class="rail" :class="{ 'rail--collapsed': railCollapsed }">
    <div class="rail__head">
      <button
        class="rail__icon rail__icon--fold"
        type="button"
        :title="railCollapsed ? '展开标签栏' : '收起标签栏'"
        :aria-label="railCollapsed ? '展开标签栏' : '收起标签栏'"
        :aria-expanded="!railCollapsed"
        @click="toggleRail"
      >
        <component
          :is="railCollapsed ? PanelLeftOpen : PanelLeftClose"
          :size="14"
          :stroke-width="1.9"
        />
      </button>

      <button
        class="rail__icon"
        type="button"
        title="新建标签页"
        aria-label="新建标签页"
        @click="emit('new-tab')"
      >
        <Plus :size="14" :stroke-width="2" />
      </button>
    </div>

    <TransitionGroup name="tab" tag="ol" class="rail__list">
      <!-- 用标签页自己的 id 作 key，不用下标：重排时下标会变，动画就会错位 -->
      <li
        v-for="(tab, index) in tabs"
        :key="tab.id"
        draggable="true"
        @dragstart="onDragStart($event, index)"
        @dragover.prevent="onDragOver($event, index)"
        @drop.prevent="onDrop(index)"
        @dragend="onDragEnd"
      >
        <div
          class="rail__item"
          :class="{
            'rail__item--active': index === active,
            'rail__item--over': overIndex === index && dragging !== index,
            'rail__item--shake': shakeIndex === index,
          }"
          @mousedown.middle.prevent="emit('close', index)"
        >
          <button
            class="rail__pick"
            type="button"
            :title="tab.address || '新标签页'"
            @click="emit('select', index)"
            @contextmenu="onTabMenu($event, index)"
          >
            <span class="rail__initial">{{ initialOf(tab.title || tab.address) }}</span>
            <span class="rail__text">{{ tab.title || tab.address || "新标签页" }}</span>
          </button>
          <button
            class="rail__close"
            type="button"
            title="关闭"
            aria-label="关闭"
            @click="emit('close', index)"
          >
            <X :size="12" :stroke-width="2" />
          </button>
        </div>
      </li>
    </TransitionGroup>

    <!-- 设置入口：放在标签列表底下，与标签区分开（它不是一个标签） -->
    <button class="rail__entry" type="button" aria-label="设置" @click="emit('settings')">
      <Settings :size="16" :stroke-width="1.75" />
      <span class="rail__text">设置</span>
    </button>
  </aside>
</template>

<style scoped src="./rail.css"/>
