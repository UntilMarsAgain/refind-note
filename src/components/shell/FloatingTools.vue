<script setup lang="ts">
import { computed, type Component } from "vue";
import {
  ArrowDown,
  ArrowLeftRight,
  ArrowUp,
  Bug,
  Moon,
  Monitor,
  Scan,
  Sun,
} from "@lucide/vue";
import type { ThemeMode } from "../../bindings/settings.ts";

/**
 * 贴在右下角、往上堆的一组按钮。
 *
 * 只做展示与派发：限宽与深浅色由 `preferences.ts` 那边落地，滚动由持有滚动容器的
 * 渲染区负责。
 *
 * 说明气泡用全局的 `data-tip` / `.tip--left`，所以这里**不写原生 title**：
 * 两套提示会同时冒出来。
 */
const props = defineProps<{
  /** 渲染区是否限宽 */
  limited: boolean;
  /** 当前深浅色，只用来决定按钮上画哪个图标 */
  theme: ThemeMode;
  /** 调试信息框开着没有 */
  debugOpen: boolean;
}>();

const emit = defineEmits<{
  (e: "toggle-debug"): void;
  (e: "toggle-theme"): void;
  (e: "toggle-width"): void;
  (e: "scroll-top"): void;
  (e: "scroll-bottom"): void;
}>();

const THEME_ICONS: Record<ThemeMode, Component> = {
  system: Monitor,
  light: Sun,
  dark: Moon,
};

const THEME_LABELS: Record<ThemeMode, string> = {
  system: "跟随系统",
  light: "浅色",
  dark: "深色",
};

const themeIcon = computed(() => THEME_ICONS[props.theme]);
const themeTip = computed(() => `深浅色：${THEME_LABELS[props.theme]}（点击切换）`);
</script>

<template>
  <div class="tools">
    <button
      class="tool tip--left"
      type="button"
      :class="{ 'tool--on': debugOpen }"
      :data-tip="debugOpen ? '收起调试信息' : '查看调试信息'"
      :aria-label="debugOpen ? '收起调试信息' : '查看调试信息'"
      :aria-pressed="debugOpen"
      @click="emit('toggle-debug')"
    >
      <Bug :size="16" :stroke-width="1.9" />
    </button>

    <button
      class="tool tip--left"
      type="button"
      :data-tip="themeTip"
      :aria-label="themeTip"
      @click="emit('toggle-theme')"
    >
      <component :is="themeIcon" :size="16" :stroke-width="1.9" />
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
.tools {
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
  transition:
    background-color 120ms ease,
    color 120ms ease,
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

/* 开关型按钮的"开着"状态 */
.tool--on {
  border-color: var(--accent-soft);
  background: var(--accent-tint);
  color: var(--text);
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
  transition:
    transform 240ms ease,
    opacity 240ms ease;
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
