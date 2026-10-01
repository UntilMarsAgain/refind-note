<script setup lang="ts">
import { Plus } from "@lucide/vue";

/**
 * 「这篇笔记还不存在」。
 *
 * 这**不是错误状态**：内部链接指向还没建的笔记、以及地址栏里敲一个新名字，
 * 都会走到这里。所以给的是**创建入口**，不是报错。
 */
defineProps<{
  title: string;
}>();

const emit = defineEmits<{
  (e: "create"): void;
}>();
</script>

<template>
  <section class="missing">
    <h1 class="missing__title">{{ title }}</h1>
    <p class="missing__hint">此笔记尚未创建。</p>

    <button class="missing__create" type="button" @click="emit('create')">
      <Plus :size="15" :stroke-width="2.1" />
      创建此笔记
    </button>
  </section>
</template>

<style scoped>
.missing {
  padding-top: 24px;
}

.missing__title {
  margin: 0 0 6px;
  font-size: 26px;
  font-weight: 600;
  line-height: 1.35;
  overflow-wrap: anywhere;
}

.missing__hint {
  margin: 0 0 20px;
  color: var(--text-dim);
  font-size: 14px;
}

.missing__create {
  appearance: none;
  display: inline-flex;
  align-items: center;
  gap: 7px;
  padding: 7px 14px;
  border: 1px solid var(--accent-soft);
  border-radius: 6px;
  background: transparent;
  color: var(--accent-soft);
  font: inherit;
  font-size: 13.5px;
  line-height: 1.4;
  cursor: pointer;
  transition:
    background-color 120ms ease,
    color 120ms ease;
}

.missing__create:hover {
  background: var(--accent);
  color: var(--text);
}
</style>
