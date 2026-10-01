<script setup lang="ts">
/**
 * 渲染区：把当前标签页的内部数据原样画出来（验证标签页与地址解析用）。
 */
import { computed } from "vue";
import type { Mode } from "../bindings/address.ts";
import type { TabState } from "../tabs.ts";

const props = defineProps<{
  tabs: TabState[];
  active: number;
}>();

const current = computed(() => props.tabs[props.active] ?? null);

/** 命名空间一行：主命名空间没有拼写，直接在页面上写清楚 */
const namespaceText = computed(() => {
  const ns = current.value?.route?.address.namespace;
  if (!ns) {
    return "（未导航）";
  }
  return ns.spelling ? `${ns.spelling}（id：${ns.id}）` : "主命名空间";
});

/** 状态一行：`view` / `view-3` / `unlock` 这样的字样 */
function modeLabel(mode: Mode): string {
  return "ref" in mode && mode.ref ? `${mode.kind}-${mode.ref}` : mode.kind;
}

const modeText = computed(() => {
  const mode = current.value?.route?.address.mode;
  return mode ? modeLabel(mode) : "（未导航）";
});
</script>

<template>
  <main class="pane">
    <section v-if="current" class="card">
      <h2 class="card__title">当前标签页</h2>
      <p v-if="current.error" class="error">解析失败：{{ current.error }}</p>
      <dl class="fields">
        <dt>id</dt>
        <dd>{{ current.id }}</dd>
        <dt>标题</dt>
        <dd>{{ current.title || "（空）" }}</dd>
        <dt>地址栏</dt>
        <dd>{{ current.address || "（空）" }}</dd>
        <dt>规范地址</dt>
        <dd>{{ current.route?.canonical || "（未导航）" }}</dd>
        <dt>命名空间</dt>
        <dd>{{ namespaceText }}</dd>
        <dt>状态</dt>
        <dd>{{ modeText }}</dd>
        <dt>章节</dt>
        <dd>{{ current.route?.address.section || "（无）" }}</dd>
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

.error {
  margin-bottom: 10px;
  padding: 7px 10px;
  border: 1px solid var(--danger);
  border-radius: 6px;
  color: var(--danger);
  font-size: 13px;
  word-break: break-all;
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
