<script setup lang="ts">
/**
 * "从哪儿来"的一行小字：跟重定向来到这一页时，标题附近标出来源。
 *
 * 两个地方要显示同一件事（笔记页标题下方、特殊页面内容上方），所以做成一个组件 ——
 * 文案与样式只写一次。样式用内联：本项目在 NoteEditor 上吃过"scoped CSS 不生效"的亏，
 * 这种小东西不值得赌样式表。
 */
defineProps<{
  /** 要显示的那句话；空串表示不显示 */
  hint: string;
}>();
</script>

<template>
  <p
    v-if="hint"
    class="via-hint"
    :style="{
      margin: '2px 0 0',
      color: 'var(--text-dim)',
      fontSize: '12.5px',
      lineHeight: 1.6,
    }"
  >
    {{ hint }}
  </p>
</template>
