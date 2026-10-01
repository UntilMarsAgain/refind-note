<script setup lang="ts">
import { TriangleAlert } from "@lucide/vue";
import { startupFailure } from "../../core/startup.ts";

/**
 * 启动没走完时顶替主区域的那张错误页。
 *
 * 标题栏留着不动，所以窗口仍然拖得动、关得掉 —— 这一条比"把错误显示得多完整"要紧。
 */
defineEmits<{
  (e: "retry"): void;
}>();
</script>

<template>
  <div class="startup">
    <div class="startup__box">
      <h1 class="startup__title">
        <TriangleAlert :size="20" :stroke-width="1.9" />
        启动没走完
      </h1>

      <p class="startup__step">在「{{ startupFailure?.step }}」这一步失败了：</p>
      <p class="startup__detail">{{ startupFailure?.detail }}</p>
      <p class="startup__hint">修好之后再试一次；也可以直接关掉窗口。</p>

      <button class="startup__retry" type="button" @click="$emit('retry')">重试</button>
    </div>
  </div>
</template>

<style scoped>
.startup {
  display: flex;
  flex: 1 1 auto;
  align-items: center;
  justify-content: center;
  min-width: 0;
  overflow: auto;
  padding: 24px;
}

.startup__box {
  max-width: min(560px, 100%);
  padding: 20px 22px;
  border: 1px solid var(--danger);
  border-left-width: 3px;
  border-radius: 10px;
  background: var(--surface);
}

.startup__title {
  display: flex;
  align-items: center;
  gap: 8px;
  margin: 0;
  color: var(--danger);
  font-size: 17px;
  font-weight: 600;
}

.startup__step {
  margin: 14px 0 0;
  color: var(--text);
  font-size: 13.5px;
}

.startup__detail {
  margin: 6px 0 0;
  color: var(--text-dim);
  font-family: var(--mono-font);
  font-size: 12.5px;
  line-height: 1.6;
  overflow-wrap: anywhere;
  -webkit-user-select: text;
  user-select: text;
}

.startup__hint {
  margin: 14px 0 0;
  color: var(--text-dim);
  font-size: 13px;
}

.startup__retry {
  appearance: none;
  margin-top: 16px;
  height: 30px;
  padding: 0 14px;
  border: 1px solid var(--accent-soft);
  border-radius: 6px;
  background: transparent;
  color: var(--accent-soft);
  font-size: 13px;
  cursor: pointer;
}

.startup__retry:hover {
  background: var(--hover);
}
</style>
