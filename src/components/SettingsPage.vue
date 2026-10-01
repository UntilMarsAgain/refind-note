<script setup lang="ts">
import { computed, nextTick, ref, watch } from "vue";
import type { Policy } from "../bindings/note.ts";
import type { ThemeMode } from "../bindings/settings.ts";
import {
  databaseRoot,
  gpgAvailable,
  preferences,
  protection,
  setProtection,
  updatePreferences,
  workspaceRoot,
} from "../preferences.ts";

/**
 * 设置页（`special:settings`）。
 *
 * 改动即时生效、随后落盘，没有"保存"按钮。
 * 每一行都带一个 id，可以直接当地址锚点：`special:settings#accent` 会跳过来并高亮。
 */
const props = defineProps<{
  /** 地址里的章节：跳到那一项并高亮 */
  focus?: string;
}>();

const THEMES: { value: ThemeMode; label: string }[] = [
  { value: "system", label: "跟随系统" },
  { value: "light", label: "浅色" },
  { value: "dark", label: "深色" },
];

/** 主题色的几个预设；第一个与默认值一致 */
const ACCENTS = [
  { label: "蓝", value: "#5b8dd6" },
  { label: "靛", value: "#7b6ee0" },
  { label: "青", value: "#3f9e9e" },
  { label: "绿", value: "#5aa469" },
  { label: "橙", value: "#c98a45" },
  { label: "红", value: "#c0575a" },
  { label: "紫", value: "#a45fbf" },
];

/** 缩放的百分比范围，与后端收口时的范围一致 */
const ZOOM_MIN = 50;
const ZOOM_MAX = 300;

/** 缩放存的是倍数，界面上用百分比 */
const zoomPercent = computed(() => Math.round(preferences.value.zoom * 100));

function submitZoom(event: Event) {
  const input = event.target as HTMLInputElement;
  const percent = Number(input.value);

  if (!Number.isFinite(percent)) {
    input.value = String(zoomPercent.value);
    return;
  }
  updatePreferences({ zoom: Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, percent)) / 100 });
}

/** 主题色输入框的草稿：只有它是合法的 `#rrggbb` 才提交 */
const accentDraft = ref(preferences.value.accent);

watch(
  () => preferences.value.accent,
  (value) => {
    accentDraft.value = value;
  },
);

function submitAccent() {
  const value = accentDraft.value.trim().toLowerCase();
  if (/^#[0-9a-f]{6}$/.test(value)) {
    updatePreferences({ accent: value });
    return;
  }
  // 写了个不成形的颜色：退回当前生效的那个，而不是把它存下去
  accentDraft.value = preferences.value.accent;
}

function toggleLimitWidth(event: Event) {
  updatePreferences({ limit_width: (event.target as HTMLInputElement).checked });
}

function toggleCodeLineNumbers(event: Event) {
  updatePreferences({ code_line_numbers: (event.target as HTMLInputElement).checked });
}

function setRailDefault(event: Event) {
  updatePreferences({ rail_collapsed: (event.target as HTMLInputElement).checked });
}

function toggleCompress(event: Event) {
  void setProtection({
    ...protection.value,
    compress: (event.target as HTMLInputElement).checked,
  });
}

function toggleSymmetric(event: Event) {
  void setProtection({
    ...protection.value,
    symmetric: (event.target as HTMLInputElement).checked,
  });
}

/** 空串 = 不用这一层，所以统一成 null */
function keyOrNothing(value: string): string | null {
  const trimmed = value.trim();
  return trimmed === "" ? null : trimmed;
}

function setSign(event: Event) {
  void setProtection({
    ...protection.value,
    gpg_sign: keyOrNothing((event.target as HTMLInputElement).value),
  });
}

function setEncrypt(event: Event) {
  void setProtection({
    ...protection.value,
    gpg_encrypt: keyOrNothing((event.target as HTMLInputElement).value),
  });
}

/** 从内到外说清这份策略会怎么存；什么都没做就是"原样" */
function policyLabel(policy: Policy): string {
  const layers: string[] = [];
  if (policy.compress) {
    layers.push("压缩");
  }
  if (policy.gpg_sign) {
    layers.push(`GPG 签名 ${policy.gpg_sign}`);
  }
  if (policy.gpg_encrypt) {
    layers.push(`GPG 加密 ${policy.gpg_encrypt}`);
  }
  if (policy.symmetric) {
    layers.push("口令加密");
  }
  return layers.length > 0 ? layers.join(" · ") : "原样";
}

function isFocused(id: string): boolean {
  return props.focus === id;
}

// 从地址里带章节跳过来时，滚到那一项
watch(
  () => props.focus,
  async (focus) => {
    if (!focus) {
      return;
    }
    await nextTick();
    document.getElementById(focus)?.scrollIntoView({ block: "center" });
  },
  { immediate: true },
);
</script>

<template>
  <section class="settings">
    <h1 class="settings__title">设置</h1>

    <!-- 东西存在哪：只读，给"备份 / 排障"时抄路径用 -->
    <dl class="places">
      <dt>工作目录</dt>
      <dd><code>{{ workspaceRoot || "…" }}</code></dd>
      <dt>数据库</dt>
      <dd><code>{{ databaseRoot || "…" }}</code></dd>
    </dl>
    <p class="settings__where">
      改动即时生效，落在工作目录下的 <code>settings/preferences.json</code>。
    </p>

    <h2 class="settings__section">外观</h2>

    <div id="theme" class="row" :class="{ 'row--target': isFocused('theme') }">
      <span class="row__label">深浅色</span>
      <code class="row__id">#theme</code>
      <div class="seg">
        <button
          v-for="theme in THEMES"
          :key="theme.value"
          type="button"
          class="seg__item"
          :class="{ 'seg__item--on': preferences.theme === theme.value }"
          @click="updatePreferences({ theme: theme.value })"
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
          :class="{ 'swatch--on': preferences.accent === accent.value }"
          :style="{ background: accent.value }"
          :title="accent.label"
          :aria-label="accent.label"
          @click="updatePreferences({ accent: accent.value })"
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

    <div id="zoom" class="row" :class="{ 'row--target': isFocused('zoom') }">
      <span class="row__label">界面缩放</span>
      <code class="row__id">#zoom</code>
      <input
        class="num"
        type="number"
        :min="ZOOM_MIN"
        :max="ZOOM_MAX"
        step="10"
        :value="zoomPercent"
        @change="submitZoom"
      />
      <span class="row__unit">%</span>
    </div>

    <h2 class="settings__section">浏览</h2>

    <div
      id="limit-width"
      class="row"
      :class="{ 'row--target': isFocused('limit-width') }"
    >
      <span class="row__label">宽度限制器</span>
      <code class="row__id">#limit-width</code>
      <label class="row__check">
        <input
          type="checkbox"
          :checked="preferences.limit_width"
          @change="toggleLimitWidth"
        />
        <span>限制阅读栏最大宽度，以便更方便地阅读</span>
      </label>
    </div>

    <div
      id="code-line-numbers"
      class="row"
      :class="{ 'row--target': isFocused('code-line-numbers') }"
    >
      <span class="row__label">代码行号</span>
      <code class="row__id">#code-line-numbers</code>
      <label class="row__check">
        <input
          type="checkbox"
          :checked="preferences.code_line_numbers"
          @change="toggleCodeLineNumbers"
        />
        <span>代码块左侧显示行号</span>
      </label>
    </div>

    <div
      id="rail-collapsed"
      class="row"
      :class="{ 'row--target': isFocused('rail-collapsed') }"
    >
      <span class="row__label">标签栏</span>
      <code class="row__id">#rail-collapsed</code>
      <label class="row__check">
        <input
          type="checkbox"
          :checked="preferences.rail_collapsed"
          @change="setRailDefault"
        />
        <span>下次打开时收起标签栏（只显示首字或图标）</span>
      </label>
    </div>

    <h2 class="settings__section">存储</h2>

    <p class="settings__note">
      这里是<strong>仓库默认</strong>的封装策略：<strong>只对还没有正文的新笔记生效</strong>。
      已经写过的笔记照自己最新一版粘住 —— 之后每次提交都沿用那一版的方式，直到提交时显式换一次。
      读的时候照每份自己的头解，所以同一个仓库里新旧封装可以并存。
      当前默认：<strong>{{ policyLabel(protection) }}</strong>
    </p>

    <div
      id="storage-compress"
      class="row"
      :class="{ 'row--target': isFocused('storage-compress') }"
    >
      <span class="row__label">压缩</span>
      <code class="row__id">#storage-compress</code>
      <label class="row__check">
        <input type="checkbox" :checked="protection.compress" @change="toggleCompress" />
        <span>落盘前先压缩（最内层，在加密之前）</span>
      </label>
    </div>

    <div
      id="storage-sign"
      class="row"
      :class="{ 'row--target': isFocused('storage-sign') }"
    >
      <span class="row__label">GPG 签名</span>
      <code class="row__id">#storage-sign</code>
      <input
        class="row__text"
        type="text"
        placeholder="签名密钥（留空 = 不签），如 me@example.com"
        :value="protection.gpg_sign ?? ''"
        :disabled="!gpgAvailable"
        @change="setSign"
      />
      <span v-if="!gpgAvailable" class="row__hint">这台计算机上没有 gpg，签名不可用</span>
    </div>

    <div
      id="storage-encrypt"
      class="row"
      :class="{ 'row--target': isFocused('storage-encrypt') }"
    >
      <span class="row__label">GPG 加密</span>
      <code class="row__id">#storage-encrypt</code>
      <input
        class="row__text"
        type="text"
        placeholder="加密到的密钥（留空 = 不加密）"
        :value="protection.gpg_encrypt ?? ''"
        :disabled="!gpgAvailable"
        @change="setEncrypt"
      />
      <span v-if="!gpgAvailable" class="row__hint">这台计算机上没有 gpg，加密不可用</span>
    </div>

    <div
      id="storage-symmetric"
      class="row"
      :class="{ 'row--target': isFocused('storage-symmetric') }"
    >
      <span class="row__label">口令加密</span>
      <code class="row__id">#storage-symmetric</code>
      <label class="row__check">
        <input type="checkbox" :checked="protection.symmetric" @change="toggleSymmetric" />
        <span>最外层再套一层口令（口令由你输入，<strong>不落盘</strong>）</span>
      </label>
      <span v-if="protection.symmetric" class="row__hint">
        新笔记提交时会问你要一个口令；以后读这些笔记也要输入它。口令不落盘，忘了就解不开。
      </span>
    </div>
  </section>
</template>

<style scoped>
.settings {
  padding: 28px 0 64px;
}

.settings__title {
  margin: 0 0 10px;
  font-size: 22px;
}

/* 工作目录 / 数据库的位置：只读的两行 */
.places {
  display: grid;
  grid-template-columns: max-content 1fr;
  gap: 2px 12px;
  margin-bottom: 8px;
  font-size: 13px;
}

.places dt {
  color: var(--text-dim);
}

.places dd code {
  padding: 1px 5px;
  border-radius: 4px;
  background: var(--hover);
  font-family: var(--mono-font);
  font-size: 12px;
  overflow-wrap: anywhere;
  /* 路径是要抄下来贴进别处的 */
  -webkit-user-select: text;
  user-select: text;
}

.settings__where {
  margin: 0 0 24px;
  color: var(--text-dim);
  font-size: 13px;
  overflow-wrap: anywhere;
}

.settings__where code {
  padding: 1px 5px;
  border-radius: 4px;
  background: var(--hover);
  font-family: var(--mono-font);
  font-size: 12px;
}

.settings__section {
  margin: 26px 0 12px;
  padding-bottom: 6px;
  border-bottom: 1px solid var(--border);
  color: var(--text-dim);
  font-size: 14px;
  font-weight: 500;
}

/* 成段说明：用来交代"这一节影响什么"，而不是塞进行标签里 */
.settings__note {
  margin: 0 0 6px;
  color: var(--text-dim);
  font-size: 13px;
  line-height: 1.75;
}

.row__text {
  flex: 1;
  min-width: 200px;
  padding: 6px 10px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: var(--surface);
  color: var(--text);
  font: inherit;
  font-size: 13px;
}

.row__text:focus {
  outline: none;
  border-color: var(--accent-soft);
}

.row__text:disabled {
  cursor: not-allowed;
  opacity: 0.55;
}

.row {
  display: flex;
  align-items: center;
  /* 窄窗口下让"标签 + 锚点 + 控件 + 单位"换行，而不是被裁掉 */
  flex-wrap: wrap;
  gap: 12px;
  margin: 10px 0;
  /* 从地址跳过来时滚到中间，别被顶边贴住 */
  scroll-margin-top: 24px;
}

.row__label {
  min-width: 132px;
  font-size: 13px;
}

/* 勾选式的一行：复选框与说明文字并排 */
.row__check {
  display: flex;
  align-items: center;
  gap: 8px;
  color: var(--text-dim);
  font-size: 13px;
  cursor: pointer;
}

.row__check input {
  accent-color: var(--accent);
  cursor: pointer;
}

.row__id {
  /* 标签本身就是给人复制用的，别被选中规则拦住 */
  -webkit-user-select: text;
  user-select: text;
  padding: 1px 6px;
  border-radius: 4px;
  background: var(--hover);
  color: var(--text-dim);
  font-family: var(--mono-font);
  font-size: 11px;
}

.row__unit {
  color: var(--text-dim);
  font-size: 12px;
}

/*
 * 控件旁的一句提醒：与标签左缘对齐另起一行，
 * 免得挤在输入框右边把它压短。
 */
.row__hint {
  flex-basis: 100%;
  padding-left: 144px;
  color: var(--text-dim);
  font-size: 12px;
}

/* 跳过来时把目标那一行点出来：左侧一条强调色 + 淡底 */
.row--target {
  margin-left: -10px;
  padding: 6px 10px;
  border-left: 3px solid var(--accent);
  border-radius: 6px;
  background: var(--accent-tint);
}

.seg {
  display: inline-flex;
  gap: 2px;
  padding: 2px;
  border: 1px solid var(--border);
  border-radius: 7px;
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

.swatches__hex,
.num {
  padding: 4px 8px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: var(--field-bg);
  color: var(--text);
  font-size: 13px;
}

.swatches__hex {
  width: 92px;
  margin-left: 6px;
  font-family: var(--mono-font);
  font-size: 12px;
}

.num {
  width: 92px;
}
</style>
