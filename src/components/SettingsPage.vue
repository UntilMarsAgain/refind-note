<script setup lang="ts">
import type { VaultSettings } from "../bindings";
import ViaHint from "./ViaHint.vue";
import { historyEnabled, setHistoryEnabled } from "../history";
import { codeLineNumbers, setCodeLineNumbers } from "../settings";
import type { Via } from "../bindings";
/**
 * 设置页（`special:settings`）。
 *
 * 它只做一件事：把 `~/.refind-note/vault.json` 里的设置集中到一个界面。
 * 改了**立刻落盘**，没有「保存」按钮 —— 这些值改错也不会有损失，多一个按钮只会
 * 多一次忘记点。
 */
import { computed, nextTick, ref, watch } from "vue";
import NamespaceManager from "./NamespaceManager.vue";
import { BASE_ZOOM } from "../settings";


const props = defineProps<{
  settings: VaultSettings;
  focus?: string;
  /** 是被哪条指令带到这一页的（null = 直接打开） */
  via: Via | null;
}>();

/** 代码行号是**界面偏好**（存这台机器上），不走后端设置 */
const lineNumbersOn = computed(() => codeLineNumbers.value);

function onLineNumbers(event: Event) {
  setCodeLineNumbers((event.target as HTMLInputElement).checked);
}

/** 浏览历史同样是**界面偏好**（存这台机器上），不走后端设置 */
const historyOn = computed(() => historyEnabled.value);

function onHistory(event: Event) {
  setHistoryEnabled((event.target as HTMLInputElement).checked);
}

/* 每个设置项的 id 是**地址的一部分**（`special:settings#accent` 能直接跳过去），
   因此它们等于对外接口：改名要同步改 id 与文案。滚动不再依赖白名单，
   凡是页面上存在的 id 都能跳（见下面的 watch）。 */

/** 地址里带了哪个 id，就把哪一项高亮出来 */
function isFocused(id: string): boolean {
  return props.focus === id;
}

// 跳过来时滚到目标项。滚动放在组件内部：id 就在这儿，不必让上层去猜。
watch(
  () => props.focus,
  async (focus) => {
    if (!focus) {
      return;
    }
    await nextTick();
    // 任何**存在于页面上**的 id 都能跳：设置项、分区，以及命名空间管理里的每一项
    // （`#namespaces`、`#ns-help` …）。找不到就什么也不做，不必维护白名单。
    document.getElementById(focus)?.scrollIntoView({ block: "center" });
  },
  { immediate: true },
);
const emit = defineEmits<{
  (e: "update", patch: Record<string, unknown>): void;
  (e: "open-via", title: string): void;
}>();

const THEMES = [
  { value: "system", label: "跟随系统" },
  { value: "light", label: "浅色" },
  { value: "dark", label: "深色" },
];

/** 几个预设；第一个与默认主题色一致 */
const ACCENTS = [
  { label: "蓝", value: "#5b8dd6" },
  { label: "靛", value: "#7b6ee0" },
  { label: "青", value: "#3f9e9e" },
  { label: "绿", value: "#5aa469" },
  { label: "橙", value: "#c98a45" },
  { label: "红", value: "#c0575a" },
  { label: "紫", value: "#a45fbf" },
];

/** 主题色输入框的草稿：只有在它成为合法的 #rrggbb 时才提交 */
const accentDraft = ref(props.settings.accent);
watch(
  () => props.settings.accent,
  (value) => {
    accentDraft.value = value;
  },
);

function submitAccent() {
  const value = accentDraft.value.trim();
  if (/^#[0-9a-fA-F]{6}$/.test(value)) {
    emit("update", { accent: value.toLowerCase() });
  } else {
    accentDraft.value = props.settings.accent;
  }
}

/**
 * 缩放用百分比输入（界面上 100 = 1.0），存进设置时换算回倍数。
 *
 * 区间与 Ctrl + 滚轮一致（50%–300%），越界就夹住并把输入框改回夹后的值 ——
 * 否则用户会看到一个"输入了却没生效"的数字。
 */
function submitZoom(event: Event) {
  const target = event.target;
  if (!(target instanceof HTMLInputElement)) {
    return;
  }
  const value = Number.parseInt(target.value, 10);
  if (!Number.isFinite(value)) {
    return;
  }
  // 区间按"生效后的百分比"夹：后端把缩放夹在 0.5–3.0，乘上基准就是 56%–336%
  const percent = Math.min(336, Math.max(56, value));
  target.value = String(percent);
  emit("update", { zoom: percent / (BASE_ZOOM * 100) });
}

function submitNumber(key: string, event: Event, min: number, max: number) {
  const target = event.target;
  if (!(target instanceof HTMLInputElement)) {
    return;
  }
  const value = Number.parseInt(target.value, 10);
  if (!Number.isFinite(value)) {
    return;
  }
  emit("update", { [key]: Math.min(max, Math.max(min, value)) });
}
</script>

<template>
  <section class="settings">
    <h1 class="settings__title">设置</h1>
    <ViaHint :via="via" @open-via="emit('open-via', $event)" />
    <p class="settings__where">
      外观存在 <code>{{ settings.root }}</code> 下的 <code>preferences.json</code>，
      存储相关的存在同目录的 <code>vault.json</code>
    </p>
    <p class="settings__where">
      每一项的 id 可直接用作地址锚点，例如 <code>special:settings#accent</code>
      会跳到主题色并高亮。
    </p>

    <h2 class="settings__section">外观</h2>

    <div id="theme" class="row" :class="{ 'row--target': isFocused('theme') }">
      <span class="row__label">主题</span>
      <code class="row__id">#theme</code>
      <div class="seg">
        <button
          v-for="theme in THEMES"
          :key="theme.value"
          type="button"
          class="seg__item"
          :class="{ 'seg__item--on': settings.theme === theme.value }"
          @click="emit('update', { theme: theme.value })"
        >
          {{ theme.label }}
        </button>
      </div>
    </div>

    <div
      id="browsing-history"
      class="row"
      :class="{ 'row--target': isFocused('browsing-history') }"
    >
      <span class="row__label">浏览历史</span>
      <code class="row__id">#browsing-history</code>
      <label class="row__check">
        <input type="checkbox" :checked="historyOn" @change="onHistory" />
        <span>记录看过的页面（在 special:history 里可以单独清空）</span>
      </label>
    </div>

    <div
      id="line-numbers"
      class="row"
      :class="{ 'row--target': isFocused('line-numbers') }"
    >
      <span class="row__label">代码行号</span>
      <code class="row__id">#line-numbers</code>
      <label class="row__check">
        <input
          type="checkbox"
          :checked="lineNumbersOn"
          @change="onLineNumbers"
        />
        <span>代码块左侧显示行号</span>
      </label>
    </div>

    <div id="accent" class="row" :class="{ 'row--target': isFocused('accent') }">
      <span class="row__label">主题色</span>
      <code class="row__id">#accent</code>
      <div class="swatches">
        <button
          v-for="accent in ACCENTS"
          :key="accent.value"
          type="button"
          class="swatch"
          :class="{ 'swatch--on': settings.accent === accent.value }"
          :style="{ background: accent.value }"
          :title="accent.label"
          :aria-label="accent.label"
          @click="emit('update', { accent: accent.value })"
        />
        <input
          v-model="accentDraft"
          class="swatches__hex"
          type="text"
          spellcheck="false"
          @change="submitAccent"
          @keydown.enter="submitAccent"
        />
      </div>
    </div>

    <div
      id="reading-width"
      class="row"
      :class="{ 'row--target': isFocused('reading-width') }"
    >
      <span class="row__label">正文限宽</span>
      <code class="row__id">#reading-width</code>
      <input
        class="num"
        type="number"
        min="480"
        max="2000"
        step="20"
        :value="settings.reading_width"
        @change="submitNumber('readingWidth', $event, 480, 2000)"
      />
      <span class="row__unit">px</span>
    </div>

    <div id="zoom" class="row" :class="{ 'row--target': isFocused('zoom') }">
      <span class="row__label">界面缩放</span>
      <code class="row__id">#zoom</code>
      <input
        class="num"
        type="number"
        min="50"
        max="300"
        step="10"
        :value="Math.round(settings.zoom * BASE_ZOOM * 100)"
        @change="submitZoom"
      />
      <span class="row__unit">%</span>
    </div>
    <p class="settings__hint">
      也可按住 Ctrl 滚轮随时调整。默认 112%（界面缩放的基准；这一项在基准之上再做增减）。
    </p>

    <h2 class="settings__section">存储</h2>

    <div
      id="delta-chain-limit"
      class="row"
      :class="{ 'row--target': isFocused('delta-chain-limit') }"
    >
      <span class="row__label">最多连续修改节点</span>
      <code class="row__id">#delta-chain-limit</code>
      <input
        class="num"
        type="number"
        min="1"
        max="256"
        :value="settings.delta_chain_limit"
        @change="submitNumber('deltaChainLimit', $event, 1, 256)"
      />
    </div>
    <p class="settings__hint">
      改动链长于这个值时，下一版改存整份快照，避免读取时逐条回放增量。
      调大更省空间、读取更慢；调小读取更快、更占空间。默认 32。
    </p>

    <div
      id="trash-keep-days"
      class="row"
      :class="{ 'row--target': isFocused('trash-keep-days') }"
    >
      <span class="row__label">回收站保留</span>
      <code class="row__id">#trash-keep-days</code>
      <input
        class="num"
        type="number"
        min="1"
        max="3650"
        :value="settings.trash_keep_days"
        @change="submitNumber('trashKeepDays', $event, 1, 3650)"
      />
      <span class="row__unit">天</span>
    </div>

    <div
      id="gc-interval-days"
      class="row"
      :class="{ 'row--target': isFocused('gc-interval-days') }"
    >
      <span class="row__label">自动回收间隔</span>
      <code class="row__id">#gc-interval-days</code>
      <input
        class="num"
        type="number"
        min="1"
        max="3650"
        :value="settings.gc_interval_days"
        @change="submitNumber('gcIntervalDays', $event, 1, 3650)"
      />
      <span class="row__unit">天</span>
    </div>
    <p class="settings__hint">
      回收站里超过保留期的条目会在启动时自动清理；自动回收按同样的方式判定：
      只看距上次执行过去了多少天，间隔之内什么都不做。下限都是 1 天。
    </p>
    <p class="settings__hint">
      上次清理回收站：{{ settings.last_trash_purge || "从未" }}<br />
      上次回收：{{ settings.last_gc || "从未" }}
    </p>
    <h2 class="settings__section">
      命名空间 <code class="row__id">#namespaces</code>
    </h2>
    <!-- id 与高亮放在**同一个元素**上：跳过来时滚到的、点亮的才是同一处 -->
    <div
      id="namespaces"
      class="ns-section"
      :class="{ 'row--target': isFocused('namespaces') }"
    >
      <NamespaceManager />
    </div>
  </section>
</template>

<style scoped>
.settings {
  /* 宽度交给 App 的阅读栏（`.app__column`）：限宽时 1080，不限宽时铺满。
     这里自设 max-width 会把它盖住，页面对"限宽"按钮就没反应了。 */
  margin: 0 auto;
  padding: 28px 20px 64px;
}

.settings__title {
  margin: 0 0 6px;
  font-size: 22px;
}

.settings__where {
  margin: 0 0 24px;
  color: var(--text-dim);
  font-size: 13px;
}

.settings__where code {
  padding: 1px 5px;
  border-radius: 4px;
  background: var(--code-bg);
  font-size: 12px;
}

/* 命名空间分区比一行设置项高得多，给它留出被粘顶标题遮住的余量 */
.ns-section {
  scroll-margin-top: 80px;
  padding: 2px 0;
}

.settings__section {
  margin: 26px 0 12px;
  padding-bottom: 6px;
  border-bottom: 1px solid var(--border);
  font-size: 14px;
  color: var(--text-dim);
  font-weight: 500;
}

.row {
  display: flex;
  align-items: center;
  /* 窄窗口下让"标签 + 锚点 + 输入框 + 单位"换行，而不是被裁掉 */
  flex-wrap: wrap;
  gap: 12px;
  margin: 10px 0;
}

.row__label {
  min-width: 132px;
  font-size: 13px;
}

/* 勾选式的一行：复选框与说明文字并排，与右边的分段控件同一位置 */
.row__check {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-left: auto;
  color: var(--text-dim);
  font-size: 13px;
  cursor: pointer;
}

.row__check input {
  accent-color: var(--accent);
  cursor: pointer;
}

.row__id {
  /* 覆盖应用级的 user-select: none —— 这个标签的用途就是让用户复制它 */
  -webkit-user-select: text;
  user-select: text;
  padding: 1px 6px;
  border-radius: 4px;
  background: var(--code-bg);
  color: var(--text-dim);
  font-family: var(--mono-font);
  font-size: 11px;
}

.row__unit {
  color: var(--text-dim);
  font-size: 12px;
}

.seg {
  display: inline-flex;
  padding: 2px;
  border: 1px solid var(--border);
  border-radius: 7px;
  gap: 2px;
}

.seg__item {
  appearance: none;
  padding: 4px 12px;
  border: 0;
  border-radius: 5px;
  background: transparent;
  color: var(--text-dim);
  font-size: 13px;
  cursor: pointer;
}

.seg__item:hover {
  background: var(--hover);
}

.seg__item--on {
  background: var(--accent);
  color: #fff;
}

.swatches {
  display: flex;
  align-items: center;
  gap: 6px;
}

.swatch {
  appearance: none;
  width: 22px;
  height: 22px;
  border: 0;
  border-radius: 6px;
  cursor: pointer;
}

.swatch--on {
  outline: 2px solid var(--text);
  outline-offset: 2px;
}

.swatches__hex {
  width: 92px;
  margin-left: 6px;
  padding: 4px 8px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: var(--field-bg);
  color: var(--text);
  font-size: 12px;
  font-family: var(--mono-font);
}

.num {
  width: 92px;
  padding: 4px 8px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: var(--field-bg);
  color: var(--text);
  font-size: 13px;
}

/* 跳过来时把目标那一项点出来：左侧一条强调色 + 淡底 */
.row--target {
  position: relative;
  margin-left: -10px;
  padding-left: 10px;
  border-radius: 7px;
  background: var(--hover);
}

.row--target::before {
  content: "";
  position: absolute;
  top: 4px;
  bottom: 4px;
  left: 0;
  width: 3px;
  border-radius: 2px;
  background: var(--accent);
}

.settings__hint {
  margin: 6px 0 0;
  color: var(--text-dim);
  font-size: 12px;
  line-height: 1.6;
}

/* ---------- 设置页的外观 ----------
 *
 * 这一节**全是覆盖**，不动上面的结构与语义：只调整"看起来"的部分，
 * 改坏了也不会影响设置项本身怎么工作。
 */

/* 收窄到一栏好读的宽度：设置项排到 1200px 宽只会显得空，控件离标签还特别远 */
.settings {
  max-width: 880px;
}

/* 标签固定宽度 —— 每一行的控件因此都从同一条竖线开始。
   "看着乱"多半来自控件起点忽左忽右，而不是颜色和圆角。 */
.row__label {
  flex: 0 0 11em;
}

.row {
  padding: 8px 10px;
  margin: 1px 0;
  border-radius: 8px;
}

/* 整行悬停时给一点底色：鼠标在长长一列里不容易跟丢自己在哪一行 */
.row:hover {
  background: var(--surface);
}

/* `#id` 是给 `special:settings#accent` 这类地址用的**锚点说明**，不是给人看的。
   藏起来，`id` 仍挂在行上、跳转照旧（用法见示例笔记的「特殊页面」一节）。 */
.row__id {
  display: none;
}

.settings__section {
  margin: 30px 0 10px;
  font-size: 15px;
  letter-spacing: 0.02em;
}

/* 说明文字缩进一点，跟它所解释的那一行对齐 */
.settings__hint {
  margin: 0 0 16px 10px;
  max-width: 64em;
}

/* 数字输入不必太长，但别贴到边上 */
.row input[type="number"] {
  padding: 5px 8px;
}

/* 复选框那一行：勾选框与文字之间别挤在一起 */
.row__check {
  gap: 8px;
  /* 原来这里是 `margin-left: auto`，于是复选框被顶到整行最右边 —— 与别的控件
     （主题按钮、数字输入）不在同一条竖线上，看着像掉队了。现在跟它们对齐。 */
  margin-left: 0;
}
</style>
