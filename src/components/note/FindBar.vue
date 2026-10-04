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

<style scoped src="./find-bar.css"/>

<!--
  页内查找的输入条：贴着渲染区右上角浮着。

  它**自己不做查找** —— 只管输入框与那几个按钮；真正去正文里找的是
  `dom/find-in-page.ts`。这么分是因为查找状态得跟着正文走
  （正文换页、视图被 KeepAlive 停用时都得重置），而输入条该跟着界面走。

  挂在哪儿由父组件决定（`RenderPane`），所以这一层不用管滚动与定位。
-->

<script setup lang="ts">
import { nextTick, ref, useTemplateRef, watch } from "vue";
import { Search, X } from "@lucide/vue";

/** 输入框：每次进来都要有焦点，否则"打开了却还要再点一下" */
const inputEl = useTemplateRef<HTMLInputElement>("inputEl");

const props = defineProps<{
  /** 开着没有（父组件控制显隐） */
  open: boolean;
  /** 命中的总数（"3/12"那个分母；0 = 没找到） */
  total: number;
  /** 当前是第几个（1 起；没命中时是 0） */
  index: number;
  /** 区分大小写勾上了没有 */
  caseSensitive: boolean;
  /** 只匹配整词勾上了没有 */
  wholeWord: boolean;
}>();

const emit = defineEmits<{
  /** 输入变了：重新查一遍 */
  (e: "query", value: string): void;
  (e: "next"): void;
  (e: "previous"): void;
  (e: "close"): void;
  /** 勾了"大小写" */
  (e: "case-sensitive", on: boolean): void;
  /** 勾了"整词" */
  (e: "whole-word", on: boolean): void;
  /** 改了选项：重查（回到第一个） */
  (e: "refresh"): void;
}>();

/** 输入框里那串字 */
const typed = ref("");

/**
 * 两个选项勾上了没有（父组件持有 —— 状态与查询词住在一起，见 `useFindInPage`）。
 *
 * 用 `props` 而不是就地改：这一层只负责显示与派发，
 * 状态换到别处重查时不会有两个来源打架。
 */
function onCaseSensitive(event: Event) {
  emit("case-sensitive", (event.target as HTMLInputElement).checked);
  emit("refresh");
}

function onWholeWord(event: Event) {
  emit("whole-word", (event.target as HTMLInputElement).checked);
  emit("refresh");
}

// 一打开就把光标放进输入框、全选好（重开时旧的查询还在框里，直接改）
watch(
  () => props.open,
  (open) => {
    if (!open) {
      return;
    }
    void nextTick(() => {
      const el = inputEl.value;
      if (el) {
        el.focus();
        el.select();
      }
    });
  },
);

/** Esc 关闭；Enter 下一个；Shift+Enter 上一个 */
function onKeydown(event: KeyboardEvent) {
  if (event.key === "Escape") {
    event.preventDefault();
    emit("close");
    return;
  }
  if (event.key === "Enter") {
    // 换行不该进这里：查找是一行的
    event.preventDefault();
    // 分开写两条：拼出来的联合类型过不了 emit 的重载签名
    if (event.shiftKey) {
      emit("previous");
    } else {
      emit("next");
    }
  }
}
</script>

<template>
  <div v-if="open" class="findbar" role="search">
    <Search class="findbar__icon" :size="14" :stroke-width="1.9" />

    <input
      ref="inputEl"
      v-model="typed"
      class="findbar__input"
      type="text"
      placeholder="在正文里查找"
      aria-label="在正文里查找"
      @input="emit('query', typed)"
      @keydown="onKeydown"
    />

    <span class="findbar__count" :class="{ 'findbar__count--none': total === 0 }">
      {{ total === 0 ? (typed.trim() ? "无" : "") : `${index}/${total}` }}
    </span>

    <!-- 两个选项：与浏览器查找同名同义（默认都不开）。
         勾一下立刻重查 —— 先找到、再挑，比"设完再找"省事。

         文字用"大小写""整词"而不是 `Aa` / `[词]` 那一类符号：这个程序通篇中文，
         而图标式的写法旁边又没有解释，看不出是干什么的。 -->
    <label class="findbar__opt" title="只找大小写完全一样的">
      <input
        type="checkbox"
        :checked="props.caseSensitive"
        @change="onCaseSensitive"
      />
      <span>大小写</span>
    </label>

    <label class="findbar__opt" title="只匹配整个词，不匹配词的一部分">
      <input
        type="checkbox"
        :checked="props.wholeWord"
        @change="onWholeWord"
      />
      <span>整词</span>
    </label>

    <button
      class="findbar__btn tip--left"
      type="button"
      data-tip="上一个"
      aria-label="上一个"
      :disabled="total === 0"
      @click="emit('previous')"
    >
      ↑
    </button>
    <button
      class="findbar__btn tip--left"
      type="button"
      data-tip="下一个"
      aria-label="下一个"
      :disabled="total === 0"
      @click="emit('next')"
    >
      ↓
    </button>
    <button
      class="findbar__btn tip--left"
      type="button"
      data-tip="关闭"
      aria-label="关闭查找"
      @click="emit('close')"
    >
      <X :size="14" :stroke-width="1.9" />
    </button>
  </div>
</template>