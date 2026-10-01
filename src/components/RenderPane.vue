<script setup lang="ts">
/**
 * 渲染区（临时版）。
 *
 * 它只做一件事：**把当前标签页的内部数据原样画出来** —— 地址栏回车、
 * 前进 / 后退、切标签页、拖动重排，都会在这里即时反映，
 * 用来肉眼验证标签页导航真的发生了。
 *
 * 等地址解析与数据模型定稿后，这里会按解析结果分发真正的视图
 * （阅读 / 编辑 / 历史 / 删除确认……）。
 */
import { computed } from "vue";
import type { TabState } from "../tabs.ts";

const props = defineProps<{
  tabs: TabState[];
  active: number;
}>();

const current = computed(() => props.tabs[props.active] ?? null);
</script>

<template>
  <main class="pane">
    <section v-if="current" class="card">
      <h2 class="card__title">当前标签页</h2>
      <dl class="fields">
        <dt>id</dt>
        <dd>{{ current.id }}</dd>
        <dt>标题</dt>
        <dd>{{ current.title || "（空）" }}</dd>
        <dt>地址栏</dt>
        <dd>{{ current.address || "（空）" }}</dd>
        <dt>游标</dt>
        <dd>{{ current.cursor }}（历史共 {{ current.history.length }} 条）</dd>
      </dl>

      <h3 class="card__subtitle">浏览历史（前进 / 后退的依据）</h3>
      <p v-if="current.history.length === 0" class="muted">
        还没去过任何地方 —— 在地址栏敲一条地址回车试试。
      </p>
      <ol v-else class="rows">
        <li
            v-for="(entry, index) in current.history"
            :key="index"
            class="row"
            :class="{ 'row--cursor': index === current.cursor }"
        >
          <span class="row__tag">{{ index }}</span>
          <span class="row__text">{{ entry }}</span>
          <span v-if="index === current.cursor" class="row__mark">← 游标</span>
        </li>
      </ol>
    </section>

    <section class="card">
      <h2 class="card__title">全部标签页（{{ tabs.length }}）</h2>
      <ol class="rows">
        <li
            v-for="(tab, index) in tabs"
            :key="tab.id"
            class="row row--dim"
            :class="{ 'row--cursor': index === active }"
        >
          <span class="row__tag">{{ tab.id }}</span>
          <span class="row__text">{{ tab.title }}</span>
          <span v-if="index === active" class="row__mark">← 当前</span>
        </li>
      </ol>
    </section>

    <p class="muted">
      临时渲染区：等地址解析与视图分发接上后，这里会被真正的页面替换。
    </p>
  </main>
</template>

<style scoped>
.pane {
  display: flex;
  flex: 1 1 auto;
  flex-direction: column;
  gap: 14px;
  min-width: 0;
  padding: 18px 22px;
  overflow-y: auto;
  color: var(--text);
}

.card {
  flex: 0 0 auto;
  padding: 14px 16px;
  border: 1px solid var(--border);
  border-radius: 10px;
  background: var(--surface);
}

.card__title {
  margin-bottom: 10px;
  font-size: 15px;
  font-weight: 600;
}

.card__subtitle {
  margin: 14px 0 6px;
  color: var(--text-dim);
  font-size: 13px;
}

.fields {
  display: grid;
  grid-template-columns: max-content 1fr;
  gap: 4px 14px;
  font-size: 13px;
}

.fields dt {
  color: var(--text-dim);
}

.fields dd {
  font-family: var(--mono-font);
  word-break: break-all;
}

.rows {
  font-family: var(--mono-font);
  font-size: 13px;
}

.row {
  display: flex;
  gap: 10px;
  align-items: baseline;
  padding: 3px 8px;
  border-radius: 6px;
}

.row--dim {
  color: var(--text-dim);
}

.row--cursor {
  background: var(--accent-tint);
  color: var(--text);
}

.row__tag {
  flex: 0 0 auto;
  width: 4.5em;
  color: var(--text-dim);
  text-align: right;
}

.row__text {
  word-break: break-all;
}

.row__mark {
  flex: 0 0 auto;
  color: var(--accent-soft);
  font-size: 12px;
}

.muted {
  color: var(--text-dim);
  font-size: 12.5px;
}
</style>
