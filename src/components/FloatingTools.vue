<script setup lang="ts">
import { ArrowDown, ArrowLeftRight, ArrowUp, Pencil, Scan } from "@lucide/vue";

/**
 * 悬浮在右下角的一组页面工具。
 *
 * 造型对齐参考：贴右下角往上堆的圆形按钮，悬停时在左侧弹出说明。
 * 说明气泡用全局的 data-tip / .tip--left，不在这里重复实现。
 *
 * 这里只做展示与派发，实际动作（限宽、滚动）由 App.vue 负责——
 * 滚动容器和限宽状态都在它手上。
 */

defineProps<{
  /** 正文是否限制为阅读栏宽度 */
  limited: boolean;
  /**
   * 当前页是否有一篇可编辑的笔记。
   *
   * 由上层判定：特殊页面、还不存在的页面、以及正在编辑时都为 false ——
   * 组件不该自己去看地址。
   */
  canEdit: boolean;
}>();

const emit = defineEmits<{
  (e: "edit"): void;
  (e: "toggle-width"): void;
  (e: "scroll-top"): void;
  (e: "scroll-bottom"): void;
}>();
</script>

<template>
  <!-- 不用原生 title：说明由 data-tip 自绘，两套同时出现会重影 -->
  <div class="floating-tools">
    <!--
      「编辑」放在最下面（离角落最近）：它是这组里最常用的动作，
      而这一组是贴着右下角往上堆的。非特殊页面都会出现，由 canEdit 决定。
    -->
    <button
      v-if="canEdit"
      class="tool tip--left"
      type="button"
      data-tip="编辑这篇"
      aria-label="编辑这篇"
      @click="emit('edit')"
    >
      <Pencil :size="16" :stroke-width="1.9" />
    </button>

    <button
      class="tool tip--left"
      type="button"
      :data-tip="limited ? '取消宽度限制' : '限制正文宽度'"
      :aria-label="limited ? '取消宽度限制' : '限制正文宽度'"
      :aria-pressed="limited"
      @click="emit('toggle-width')"
    >
      <!-- 两个图标叠在一起，靠旋转 + 淡入淡出来回切 -->
      <span class="swap" :class="{ 'swap--wide': !limited }">
        <Scan class="swap__icon swap__icon--limited" :size="16" :stroke-width="1.9" />
        <ArrowLeftRight class="swap__icon swap__icon--wide" :size="16" :stroke-width="1.9" />
      </span>
    </button>

    <button
      class="tool tip--left"
      type="button"
      data-tip="滚动至页顶"
      aria-label="滚动至页顶"
      @click="emit('scroll-top')"
    >
      <ArrowUp :size="16" :stroke-width="1.9" />
    </button>

    <button
      class="tool tip--left"
      type="button"
      data-tip="滚动至页底"
      aria-label="滚动至页底"
      @click="emit('scroll-bottom')"
    >
      <ArrowDown :size="16" :stroke-width="1.9" />
    </button>
  </div>
</template>

<style scoped>
.floating-tools {
  position: fixed;
  right: 16px;
  bottom: 28px;
  z-index: 30;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.tool {
  position: relative;
  appearance: none;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 34px;
  height: 34px;
  padding: 0;
  border: 1px solid var(--border);
  border-radius: 50%;
  /* 不透明：宽模式下万一有内容贴到边上，也不会糊在图标底下 */
  background: var(--bg);
  color: var(--text-dim);
  cursor: pointer;
  transition: background-color 120ms ease, color 120ms ease,
    border-color 120ms ease;
}

.tool:hover {
  border-color: var(--accent-soft);
  background: var(--hover);
  color: var(--text);
}

.tool:active {
  background: var(--press);
}

/* ---------- 限宽按钮的图标切换动画 ---------- */

.swap {
  position: relative;
  display: block;
  width: 16px;
  height: 16px;
}

.swap__icon {
  position: absolute;
  inset: 0;
  transition: transform 240ms ease, opacity 240ms ease;
}

/* 限宽中：Scan 在位，双向箭头收起 */
.swap__icon--limited {
  opacity: 1;
  transform: rotate(0deg) scale(1);
}

.swap__icon--wide {
  opacity: 0;
  transform: rotate(-90deg) scale(0.6);
}

/* 取消限宽后翻过来 */
.swap--wide .swap__icon--limited {
  opacity: 0;
  transform: rotate(90deg) scale(0.6);
}

.swap--wide .swap__icon--wide {
  opacity: 1;
  transform: rotate(0deg) scale(1);
}

@media (prefers-reduced-motion: reduce) {
  .swap__icon {
    transition: none;
  }
}
</style>
