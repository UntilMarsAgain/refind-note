<script setup lang="ts">
/**
 * 设置页（`special:settings`）。
 *
 * 它只做一件事：把 `~/.refind-note/vault.json` 里的设置集中到一个界面。
 * 改了**立刻落盘**，没有「保存」按钮 —— 这些值改错也不会有损失，多一个按钮只会
 * 多一次忘记点。
 */
import { nextTick, ref, watch } from "vue";
import { BASE_ZOOM } from "../settings";

interface Settings {
  root: string;
  format: number;
  capital_links: boolean;
  max_title_bytes: number;
  delta_chain_limit: number;
  theme: string;
  accent: string;
  reading_width: number;
  /** 界面缩放（1.0 = 100%） */
  zoom: number;
}

const props = defineProps<{ settings: Settings; focus?: string }>();

/**
 * 当前运行的前端产物文件名（含内容哈希）。
 *
 * 没有 devtools 时，这是判断"看到的是不是最新构建"的唯一可靠依据 ——
 * 把它和构建日志里的文件名对一下即可。
 */
const bundleName = import.meta.url.split("/").pop() ?? "";

/**
 * 每个设置项的 id 是**地址的一部分**（`special:settings#accent` 能直接跳过去），
 * 因此它们等于对外接口：改名要同步改这里的 id 与文案。
 */
const SECTION_IDS = [
  "theme",
  "accent",
  "reading-width",
  "zoom",
  "delta-chain-limit",
];

/** 地址里带了哪个 id，就把哪一项高亮出来 */
function isFocused(id: string): boolean {
  return props.focus === id;
}

// 跳过来时滚到目标项。滚动放在组件内部：id 就在这儿，不必让上层去猜。
watch(
  () => props.focus,
  async (focus) => {
    if (!focus || !SECTION_IDS.includes(focus)) {
      return;
    }
    await nextTick();
    document.getElementById(focus)?.scrollIntoView({ block: "center" });
  },
  { immediate: true },
);
const emit = defineEmits<{ (e: "update", patch: Record<string, unknown>): void }>();

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
    <p class="settings__where">
      外观存在 <code>{{ settings.root }}</code> 下的 <code>preferences.json</code>，
      存储相关的存在同目录的 <code>vault.json</code>
    </p>
    <p class="settings__where">
      每一项的 id 可直接用作地址锚点，例如 <code>special:settings#accent</code>
      会跳到主题色并高亮。
    </p>

    <p class="settings__where">界面版本：<code>{{ bundleName }}</code></p>

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
  </section>
</template>

<style scoped>
.settings {
  max-width: 640px;
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
  gap: 12px;
  margin: 10px 0;
}

.row__label {
  min-width: 132px;
  font-size: 13px;
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
</style>
