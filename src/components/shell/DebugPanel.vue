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
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { Copy, X } from "@lucide/vue";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { canonicalOf, sectionOf } from "../../core/address.ts";
import type { Mode, ResolvedAddress } from "../../ipc/address.ts";
import { THEME_LABELS } from "../../ipc/settings.ts";
import { flash } from "../../core/notice.ts";
import { preferences, railCollapsed } from "../../core/preferences.ts";
import { resolvedTheme } from "../../core/theme.ts";
import { labelOf } from "../../core/special.ts";
import type { TabState } from "../../core/tabs.ts";

/**
 * 调试信息框：**前端此刻**与**当前这一页**。
 *
 * 右下角那组按钮里的「虫子」调它出来。它只读，不改任何东西 ——
 * 排查"看着不对"的时候，先看这里比到处打日志快。
 *
 * **分工**（这条线要守住，不然两块很快就长得一样）：
 *
 * - 这里（浮动）：眼前这一页（地址、解析结果、本页历史、页面上数出来的东西）
 *   与前端此刻的样子（缩放、深浅色、窗口尺寸、标签栏开合）；
 * - `special:debug`（整页）：**软件整体与仓库** —— 工作目录、数据库、偏好默认值、
 *   仓库规模、同步配置、渲染能力、系统集成。
 *
 * 一句话：**换一页就该变的东西留在这里，换一页不变的去那张诊断页**。
 */
const props = defineProps<{
  tab: TabState | null;
}>();

defineEmits<{
  (e: "close"): void;
}>();

const route = computed<ResolvedAddress | null>(() => props.tab?.route ?? null);

/** 窗口尺寸：版式出问题时它就是第一条线索 */
const windowSize = ref(`${window.innerWidth} × ${window.innerHeight}`);

function onResize() {
  windowSize.value = `${window.innerWidth} × ${window.innerHeight}`;
}

/**
 * 页面上数出来的东西。
 *
 * 这几样正好都是最近加的：公式、图、图片、加密附件。**"没显示出来"的时候先看这里** ——
 * 是压根**没认出来**（总数 0），还是**认出来了没画出来**（总数有、画成的 0）：
 * 前者去看后端（语法认没认），后者去看前端（引擎能不能加载）。
 *
 * 数的是**渲染完的 DOM**（`decorateNoteHtml` 收尾之后的样子），不是笔记原文。
 */
const pageStats = ref("（还没数）");

function countPage(): void {
  const body = document.querySelector(".note-body");
  if (!body) {
    pageStats.value = "（这一页没有正文）";
    return;
  }

  const count = (selector: string) => body.querySelectorAll(selector).length;
  const formulas = count(".math");
  const drawnFormulas = count(".math[data-math-ready]");
  const diagrams = count("pre.mermaid");
  const drawnDiagrams = count('pre.mermaid[data-processed="true"]');
  const brokenDiagrams = count("pre.mermaid[data-problem]");
  const images = count("img");
  const missingImages = count(".image-missing");
  const locked = count(".file-locked");

  pageStats.value = [
    `公式 ${drawnFormulas}/${formulas}`,
    `图 ${drawnDiagrams}/${diagrams}${brokenDiagrams > 0 ? `（画不出来 ${brokenDiagrams}）` : ""}`,
    `图片 ${images}${missingImages > 0 ? `（取不到 ${missingImages}）` : ""}`,
    `加密附件 ${locked}`,
  ].join("，");
}

let observer: MutationObserver | undefined;
let recount: number | undefined;

onMounted(() => {
  window.addEventListener("resize", onResize);
  countPage();

  // 正文是**注入**进来的，注入之后还会变（公式排版、图换成 SVG）：盯着一改就重数。
  // 攒 200ms 再数 —— 编辑器预览每敲一个字就会重注入一次。
  observer = new MutationObserver(() => {
    window.clearTimeout(recount);
    recount = window.setTimeout(countPage, 200);
  });
  observer.observe(document.body, {
    childList: true,
    subtree: true,
    attributes: true,
    // 只盯"画好了没有"这几个记号，别把整棵树的属性变化都收进来
    attributeFilter: ["data-math-ready", "data-processed", "data-problem"],
  });
});

onBeforeUnmount(() => {
  window.removeEventListener("resize", onResize);
  observer?.disconnect();
  window.clearTimeout(recount);
});

/** 地址里的浏览状态，按它自己的写法显示（与规范串里的后缀一致） */
function modeLabel(mode: Mode | null): string {
  if (!mode) {
    return "（无）";
  }
  switch (mode.kind) {
    case "view":
      return mode.ref ? `view-${mode.ref}` : "view";
    case "rollback":
      return `rollback-${mode.ref}`;
    case "unlock":
      return mode.ref ? `unlock-${mode.ref}` : "unlock";
    case "no-command":
      return "no-command";
    default:
      return mode.kind;
  }
}

/** 地址落到仓库上的结论说成一句话 */
function outcomeLabel(resolved: ResolvedAddress | null): string {
  if (!resolved) {
    return "新标签页：还没有地址";
  }
  switch (resolved.outcome.kind) {
    case "note":
      return `笔记「${resolved.outcome.title}」`;
    case "missing":
      return `「${resolved.outcome.title}」还不存在`;
    case "special":
      return `特殊页面「${labelOf(resolved.outcome.page)}」`;
    case "help":
      return `帮助页「${resolved.outcome.title}」`;
    case "cross-site":
      return `跨站页面「${resolved.outcome.title}」（本仓库没有它）`;
    case "file":
      return `文件「${resolved.outcome.title}」`;
  }
}

/**
 * 一段段的事实。
 *
 * **渲染与复制用的是同一份** —— 否则"看到的"和"抄走的"迟早对不上。
 */
const sections = computed(() => {
  const tab = props.tab;
  return [
    {
      title: "这一页",
      rows: [
        { label: "解析结果", value: outcomeLabel(route.value) },
        { label: "地址模式", value: modeLabel(route.value?.address.mode ?? null) },
        { label: "规范地址", value: canonicalOf(route.value) || "（无）" },
        { label: "地址章节", value: sectionOf(route.value) || "（无）" },
        {
          label: "本页历史",
          value: tab?.history.length
            ? `${tab.cursor + 1} / ${tab.history.length}`
            : "（还没去过任何地方）",
        },
        { label: "页面上有", value: pageStats.value },
      ],
    },
    {
      title: "前端此刻",
      rows: [
        { label: "地址栏内容", value: tab?.address || "（空）" },
        { label: "缩放", value: `${Math.round(preferences.value.zoom * 100)}%` },
        {
          label: "深浅色",
          value: `${THEME_LABELS[preferences.value.theme]}（此刻${
            resolvedTheme.value === "light" ? "浅色" : "深色"
          }）`,
        },
        { label: "主题色", value: preferences.value.accent },
        { label: "宽度限制器", value: preferences.value.limit_width ? "开" : "关" },
        { label: "标签栏（此刻）", value: railCollapsed.value ? "收起" : "展开" },
        { label: "窗口", value: `${windowSize.value} · 像素比 ${window.devicePixelRatio}` },
      ],
    },
  ];
});

/** 抄成「分组 / 标签：值」—— 贴进别处时不用再整理 */
async function copyFacts() {
  const text = sections.value
    .map((section) =>
      [`【${section.title}】`, ...section.rows.map((row) => `${row.label}：${row.value}`)].join("\n"),
    )
    .join("\n\n");

  try {
    await writeText(text);
    flash("调试信息已复制");
  } catch (error) {
    console.warn("复制调试信息失败：", error);
    flash(`复制失败：${error}`);
  }
}
</script>

<template>
  <section class="debug" role="region" aria-label="调试信息">
    <header class="debug__head">
      <span class="debug__title">调试信息</span>
      <span class="debug__actions">
        <button class="debug__icon" type="button" title="复制全部" aria-label="复制全部" @click="copyFacts">
          <Copy :size="13" :stroke-width="2"/>
        </button>
        <button class="debug__icon" type="button" title="关闭" aria-label="关闭" @click="$emit('close')">
          <X :size="13" :stroke-width="2"/>
        </button>
      </span>
    </header>

    <div class="debug__body">
      <section v-for="section in sections" :key="section.title" class="debug__section">
        <h2 class="debug__heading">{{ section.title }}</h2>
        <dl class="debug__list">
          <div v-for="row in section.rows" :key="row.label" class="debug__row">
            <dt class="debug__label">{{ row.label }}</dt>
            <dd class="debug__value">{{ row.value }}</dd>
          </div>
        </dl>
      </section>
    </div>
  </section>
</template>

<style scoped>
.debug {
  display: flex;
  flex-direction: column;
  width: min(420px, 72vw);
  max-height: min(66vh, 520px);
  border: 1px solid var(--border);
  border-radius: 10px;
  /* 不透明：它压在渲染区上，半透明会让底下的字透上来 */
  background: var(--surface);
  box-shadow: 0 12px 34px rgb(0 0 0 / 28%);
}

.debug__head {
  display: flex;
  flex: 0 0 auto;
  align-items: center;
  justify-content: space-between;
  gap: 10px;
  padding: 8px 8px 8px 12px;
  border-bottom: 1px solid var(--border);
}

.debug__title {
  color: var(--text-dim);
  font-size: 12.5px;
  letter-spacing: 0.02em;
}

.debug__actions {
  display: inline-flex;
  gap: 2px;
}

.debug__icon {
  appearance: none;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 22px;
  height: 22px;
  padding: 0;
  border: 0;
  border-radius: 5px;
  background: transparent;
  color: var(--text-dim);
  cursor: pointer;
}

.debug__icon:hover {
  background: var(--hover);
  color: var(--text);
}

.debug__body {
  flex: 1 1 auto;
  min-height: 0;
  padding: 4px 12px 12px;
  overflow-y: auto;
}

.debug__section {
  margin-top: 12px;
}

.debug__heading {
  margin: 0 0 6px;
  color: var(--text-dim);
  font-size: 11.5px;
  font-weight: 600;
  letter-spacing: 0.02em;
}

.debug__list {
  margin: 0;
  border: 1px solid var(--border);
  border-radius: 8px;
  overflow: hidden;
}

.debug__row {
  display: grid;
  grid-template-columns: minmax(78px, 0.34fr) 1fr;
  gap: 10px;
  padding: 5px 10px;
  /* 偶数行淡淡分一下，一屏里才看得清哪一行配哪一行 */
  background: var(--bg);
}

.debug__row:nth-child(even) {
  background: var(--surface);
}

.debug__label {
  margin: 0;
  color: var(--text-dim);
  font-size: 11.5px;
}

.debug__value {
  margin: 0;
  color: var(--text);
  font-size: 12px;
  /* 值里可能是长地址：允许换行，并让长串断开而不是撑破面板 */
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  /* 这些值是要抄下来贴进别处的 */
  -webkit-user-select: text;
  user-select: text;
}
</style>
