<script setup lang="ts">
/**
 * "从哪儿来"的一行小字：跟重定向来到这一页时，标题附近标出来源。
 *
 * **来源可点**：点它去 `原页@no-command` —— 打开那一页本身，而不是再执行一次它的指令。
 * 否则点一下又被带走，等于什么都没看到。
 *
 * 两个地方用它（笔记页标题下方、特殊页面标题下方），所以做成一个组件：
 * 文案、样式与点击行为都只写一次。样式用内联：本项目在 NoteEditor 上吃过
 * "scoped CSS 不生效"的亏，这种小东西不值得赌样式表。
 */
import type { Via } from "../bindings";

defineProps<{
  /** 来源；null 表示不是被带过来的（这一行不显示） */
  via: Via | null;
}>();

const emit = defineEmits<{ (e: "open-via", title: string): void }>();
</script>

<template>
  <p
    v-if="via"
    class="via-hint"
    :style="{
      margin: '2px 0 0',
      color: 'var(--text-dim)',
      fontSize: '12.5px',
      lineHeight: 1.6,
    }"
  >
    <!-- 随机跳转没有"来源页"这回事：发起随机的是一页指令，不是内容页 -->
    <template v-if="via.random">来自随机重定向</template>
    <template v-else>
      重定向自
      <button
        class="via-hint__link"
        type="button"
        :title="'打开《' + via.from + '》本身（不执行它的指令）'"
        :style="{
          padding: 0,
          border: 0,
          background: 'none',
          color: 'var(--link-blue)',
          font: 'inherit',
          cursor: 'pointer',
        }"
        @click="emit('open-via', via.from)"
      >
        《{{ via.from }}》
      </button>
    </template>
  </p>
</template>
