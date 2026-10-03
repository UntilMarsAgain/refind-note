<!--
  Refind Note is a note-taking software.
  Copyright (C) 2026 Until Mars Again

  This program is free software: you can redistribute it and/or modify
  it under the terms of the GNU Affero General Public License as published by
  the Free Software Foundation, either version 3 of the License, or
  (at your option) any later version.

  This program is distributed in the hope that it will be useful,
  but WITHOUT ANY WARRANTY; without even the implied warranty of
  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
  GNU Affero General Public License for more details.

  You should have received a copy of the GNU Affero General Public License
  along with this program.  If not, see <http://www.gnu.org/licenses/>.
-->

<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { preferences, updatePreferences } from "../../core/preferences.ts";
import type { ThemeMode } from "../../ipc/settings.ts";

/**
 * 设置页的「外观」与「浏览」两节（`#theme` / `#accent` / `#zoom` / `#limit-width` …）。
 *
 * 为什么这两节合在一个组件里：它们六项**全都是偏好**（`preferences.json`，
 * 跟这台机器走），改法也只有一种 —— 改完立刻生效、随后节流落盘。
 * 和后面几节比，它们不碰仓库（维护、封装策略）、也不碰同步，各自成一块反而更碎。
 *
 * 缩放是唯一要收口的一个：存的是**倍数**，界面上是百分比，越界的那部分在这一层夹住。
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

function isFocused(id: string): boolean {
  return props.focus === id;
}
</script>

<template>
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
      <span>下次启动时收起标签栏（仅显示图标）</span>
    </label>
  </div>
</template>

<style scoped src="./rows.css"/>
