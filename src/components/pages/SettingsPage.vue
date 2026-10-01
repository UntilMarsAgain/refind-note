<script setup lang="ts">
import { computed, nextTick, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { flash } from "../../core/notice.ts";
import type { Policy } from "../../ipc/note.ts";
import KeyChooser from "../common/KeyChooser.vue";
import NamespaceManager from "./NamespaceManager.vue";
import type { ThemeMode } from "../../ipc/settings.ts";
import {
  databaseRoot,
  gpgAvailable,
  maintenance,
  preferences,
  protection,
  lockAll,
  refreshWorkspaceInfo,
  setProtection,
  updatePreferences,
  workspaceRoot,
} from "../../core/preferences.ts";
import { formatTime } from "../../ipc/maintenance.ts";

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

/** 改整理设置：两个天数一起提交（它俩都在仓库的设置里） */
async function submitMaintenance(patch: { trash?: number; gc?: number }) {
  const trash = patch.trash ?? maintenance.value.trash_keep_days;
  const gc = patch.gc ?? maintenance.value.gc_interval_days;

  try {
    await invoke("set_maintenance", {
      trashKeepDays: Math.max(1, Math.round(trash)),
      gcIntervalDays: Math.max(1, Math.round(gc)),
    });
    await refreshWorkspaceInfo();
  } catch (reason) {
    console.warn("改整理设置失败：", reason);
    flash(String(reason));
  }
}

function submitKeepDays(event: Event) {
  const input = event.target as HTMLInputElement;
  const days = Number(input.value);
  if (!Number.isFinite(days)) {
    input.value = String(maintenance.value.trash_keep_days);
    return;
  }
  void submitMaintenance({ trash: days });
}

function submitGcInterval(event: Event) {
  const input = event.target as HTMLInputElement;
  const days = Number(input.value);
  if (!Number.isFinite(days)) {
    input.value = String(maintenance.value.gc_interval_days);
    return;
  }
  void submitMaintenance({ gc: days });
}

function toggleLimitWidth(event: Event) {
  updatePreferences({ limit_width: (event.target as HTMLInputElement).checked });
}

function toggleCodeLineNumbers(event: Event) {
  updatePreferences({ code_line_numbers: (event.target as HTMLInputElement).checked });
}

/** 忘掉这次会话里存过的**全部**口令（口令从不落盘，所以这就是"锁上"） */
async function lockEverything() {
  try {
    await lockAll();
    flash("已清除本次会话中的全部口令，阅读加密内容时需重新输入");
  } catch (reason) {
    flash(`锁定失败：${reason}`);
  }
}

function toggleHistory(event: Event) {
  updatePreferences({ record_history: (event.target as HTMLInputElement).checked });
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

/** 挑了签名密钥（`null` = 不签名）。默认策略跟着仓库走，改完立刻写盘 */
function setSign(value: string | null) {
  void setProtection({ ...protection.value, gpg_sign: value });
}

/** 挑了加密密钥（`null` = 不加密） */
function setEncrypt(value: string | null) {
  void setProtection({ ...protection.value, gpg_encrypt: value });
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
        <span>下次启动时收起标签栏（仅显示图标）</span>
      </label>
    </div>

    <h2 class="settings__section">命名空间</h2>

    <div
      id="namespaces"
      class="row row--stack"
      :class="{ 'row--target': isFocused('namespaces') }"
    >
      <div class="row__head">
        <span class="row__label">命名空间</span>
        <code class="row__id">#namespaces</code>
      </div>
      <NamespaceManager/>
    </div>

    <div
      id="browsing-history"
      class="row"
      :class="{ 'row--target': isFocused('browsing-history') }"
    >
      <span class="row__label">浏览历史</span>
      <code class="row__id">#browsing-history</code>
      <label class="row__check">
        <input
          type="checkbox"
          :checked="preferences.record_history"
          @change="toggleHistory"
        />
        <span>记录访问过的页面（可在浏览历史页单独清空）</span>
      </label>
    </div>

    <div
      id="passphrase"
      class="row"
      :class="{ 'row--target': isFocused('passphrase') }"
    >
      <span class="row__label">口令</span>
      <code class="row__id">#passphrase</code>
      <button type="button" class="ebtn" @click="lockEverything">
        清除本次会话中的全部口令
      </button>
      <span class="row__hint">
        口令不写入磁盘，仅保存在本次会话中。此操作会将其清除（即锁定），
        再次阅读时需重新输入。若只想清除某一篇的口令，可在该页的「口令加密」标记上操作。
      </span>
    </div>

    <h2 class="settings__section">维护</h2>

    <p class="settings__note">
      删除的笔记先进入回收站，超过保留期后由启动时的自动维护清理；
      整理仅释放不再被引用的数据。缩短保留期会使一批条目在下次启动时被清理。
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
        step="1"
        :value="maintenance.trash_keep_days"
        @change="submitKeepDays"
      />
      <span class="row__unit">天</span>
      <span class="row__hint">上次清理：{{ formatTime(maintenance.last_trash_purge) }}</span>
    </div>

    <div
      id="gc-interval-days"
      class="row"
      :class="{ 'row--target': isFocused('gc-interval-days') }"
    >
      <span class="row__label">自动整理间隔</span>
      <code class="row__id">#gc-interval-days</code>
      <input
        class="num"
        type="number"
        min="1"
        max="3650"
        step="1"
        :value="maintenance.gc_interval_days"
        @change="submitGcInterval"
      />
      <span class="row__unit">天</span>
      <span class="row__hint">上次整理：{{ formatTime(maintenance.last_gc) }}</span>
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
        <span>存储前先压缩（在签名与加密之前进行）</span>
      </label>
    </div>

    <div
      id="storage-sign"
      class="row"
      :class="{ 'row--target': isFocused('storage-sign') }"
    >
      <span class="row__label">GPG 签名</span>
      <code class="row__id">#storage-sign</code>
      <KeyChooser
        class="row__text"
        :model-value="protection.gpg_sign"
        empty-label="不签名"
        :disabled="!gpgAvailable"
        @update:model-value="setSign"
      />
      <span v-if="!gpgAvailable" class="row__hint">本机未安装 gpg，签名不可用</span>
    </div>

    <div
      id="storage-encrypt"
      class="row"
      :class="{ 'row--target': isFocused('storage-encrypt') }"
    >
      <span class="row__label">GPG 加密</span>
      <code class="row__id">#storage-encrypt</code>
      <KeyChooser
        class="row__text"
        :model-value="protection.gpg_encrypt"
        empty-label="不加密"
        :disabled="!gpgAvailable"
        @update:model-value="setEncrypt"
      />
      <span v-if="!gpgAvailable" class="row__hint">本机未安装 gpg，加密不可用</span>
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
        <span>在最外层附加口令保护（口令由你输入，<strong>不写入磁盘</strong>）</span>
      </label>
      <span v-if="protection.symmetric" class="row__hint">
        新建笔记提交时会要求设置口令，此后阅读这些笔记也需要输入。口令不写入磁盘，遗失后无法恢复。
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

/* 整块内容占一行的（命名空间表这类）：不要那两道缩进 */
.row--stack {
  display: block;
}

/* 这类行自己排头：标签 + 锚点提示，与别的行对齐 */
.row__head {
  display: flex;
  flex-wrap: wrap;
  gap: 12px;
  align-items: center;
  margin-bottom: 8px;
}

/* 设置页里偶尔要用按钮（如"忘掉口令"）：它长得像编辑器工具栏上那些 */
.ebtn {
  display: inline-flex;
  gap: 5px;
  align-items: center;
  padding: 5px 11px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: transparent;
  color: var(--text-dim);
  font: inherit;
  font-size: 13px;
  cursor: pointer;
}

.ebtn:hover {
  border-color: var(--accent-soft);
  background: var(--accent-tint);
  color: var(--text);
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
  /*
   * 定宽：锚点标签的长短不一样（`#theme` 短、`#code-line-numbers` 长），
   * 随内容撑开就会把后面的控件推到不同的位置、整页看着错位。
   */
  flex: 0 0 auto;
  width: 152px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
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
