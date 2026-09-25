<script setup lang="ts">
import { ArrowDown, ArrowLeftRight, ArrowUp, Scan } from "@lucide/vue";

/**
 * 悬浮在右下角的一组页面工具。
 *
 * 造型对齐参考：贴右下角往上堆的圆形按钮，悬停时在左侧弹出说明气泡。
 * 这里只做展示与派发，实际动作（限宽、滚动）由 App.vue 负责——
 * 滚动容器和限宽状态都在它手上。
 */

defineProps<{
  /** 正文是否限制为阅读栏宽度 */
  limited: boolean;
}>();

const emit = defineEmits<{
  (e: "toggle-width"): void;
  (e: "scroll-top"): void;
  (e: "scroll-bottom"): void;
}>();
</script>

<template>
  <div class="floating-tools">
    <!-- 图标不用 title：说明由 .tool::after 自绘，两套同时出现会重影 -->
    <button
      class="tool"
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
      class="tool"
      type="button"
      data-tip="滚动至页顶"
      aria-label="滚动至页顶"
      @click="emit('scroll-top')"
    >
      <ArrowUp :size="16" :stroke-width="1.9" />
    </button>

    <button
      class="tool"
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

/* ---------- 悬停说明气泡 ---------- */

.tool::after {
  content: attr(data-tip);
  position: absolute;
  top: 50%;
  right: calc(100% + 12px);
  padding: 5px 10px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: var(--surface);
  color: var(--text);
  font-size: 12.5px;
  line-height: 1.4;
  white-space: nowrap;
  pointer-events: none;
  opacity: 0;
  transform: translate(4px, -50%);
  transition: opacity 120ms ease, transform 120ms ease;
}

/* 指向按钮的小三角：旋转 45° 的方块，只描两条边 */
.tool::before {
  content: "";
  position: absolute;
  top: 50%;
  right: calc(100% + 8px);
  width: 8px;
  height: 8px;
  background: var(--surface);
  border-top: 1px solid var(--border);
  border-right: 1px solid var(--border);
  pointer-events: none;
  opacity: 0;
  transform: translateY(-50%) rotate(45deg);
  transition: opacity 120ms ease;
}

.tool:hover::after,
.tool:focus-visible::after {
  opacity: 1;
  transform: translate(0, -50%);
}

.tool:hover::before,
.tool:focus-visible::before {
  opacity: 1;
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
  .swap__icon,
  .tool::after,
  .tool::before {
    transition: none;
  }
}
</style>
