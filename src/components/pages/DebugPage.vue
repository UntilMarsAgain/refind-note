<script setup lang="ts">
import { computed } from "vue";
import { Copy } from "@lucide/vue";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import type { ThemeMode } from "../../bindings/settings.ts";
import { flash } from "../../core/notice.ts";
import {
  databaseMeta,
  databaseRoot,
  gpgAvailable,
  preferences,
  protection,
  workspaceRoot,
} from "../../core/preferences.ts";

/**
 * 诊断页（`special:debug`）：工作目录、数据库与当前偏好的现状。
 *
 * 报告分成几段摊开：段是"哪一块"，行是"哪一项"。**显示与整段复制用的是同一份**，
 * 否则"看到的"和"抄走的"迟早对不上。
 */

/** 没打开时统一用这个占位 */
const NOT_OPEN = "（没打开）";

const root = computed(() => workspaceRoot.value || NOT_OPEN);
const dbDir = computed(() => databaseRoot.value || NOT_OPEN);
const preferencesFile = computed(() =>
  workspaceRoot.value ? `${workspaceRoot.value}/settings/preferences.json` : NOT_OPEN,
);
const configFile = computed(() =>
  workspaceRoot.value ? `${workspaceRoot.value}/settings/config.json` : NOT_OPEN,
);

/** 深浅色的显示名 */
const THEME_LABELS: Record<ThemeMode, string> = {
  system: "跟随系统",
  light: "浅色",
  dark: "深色",
};

function yesNo(value: boolean): string {
  return value ? "是" : "否";
}

/** 一份事实，按分组摆好 */
const sections = computed(() => {
  const meta = databaseMeta.value;
  const prefs = preferences.value;
  const policy = protection.value;

  return [
    {
      title: "工作目录",
      rows: [
        { label: "位置", value: root.value },
        { label: "偏好文件", value: preferencesFile.value },
        { label: "仓库配置", value: configFile.value },
      ],
    },
    {
      title: "数据库",
      rows: [
        { label: "目录", value: dbDir.value },
        { label: "认领标记", value: meta?.kind || NOT_OPEN },
        { label: "版本", value: meta?.version || NOT_OPEN },
        { label: "建库时间", value: meta?.created_at || NOT_OPEN },
      ],
    },
    {
      title: "当前偏好",
      rows: [
        { label: "界面缩放", value: `${Math.round(prefs.zoom * 100)}%` },
        { label: "主题色", value: prefs.accent },
        { label: "深浅色", value: THEME_LABELS[prefs.theme] },
        { label: "宽度限制器", value: yesNo(prefs.limit_width) },
        { label: "标签栏默认", value: prefs.rail_collapsed ? "收起" : "展开" },
        { label: "代码行号", value: yesNo(prefs.code_line_numbers) },
      ],
    },
    {
      title: "仓库默认保护策略",
      rows: [
        { label: "压缩", value: yesNo(policy.compress) },
        { label: "GPG 签名", value: policy.gpg_sign || "不用" },
        { label: "GPG 加密", value: policy.gpg_encrypt || "不用" },
        { label: "口令加密", value: yesNo(policy.symmetric) },
      ],
    },
    {
      title: "GPG",
      rows: [{ label: "这台计算机", value: gpgAvailable.value ? "有 gpg" : "没有 gpg" }],
    },
  ];
});

/** 抄成带分组的小块：贴进别处时还看得出哪一条属于哪一组 */
async function copyFacts() {
  const text = sections.value
    .map((section) =>
      [`【${section.title}】`, ...section.rows.map((row) => `${row.label}：${row.value}`)].join("\n"),
    )
    .join("\n\n");

  try {
    await writeText(text);
    flash("诊断信息已复制");
  } catch (error) {
    console.warn("复制诊断信息失败：", error);
    flash(`复制失败：${error}`);
  }
}
</script>

<template>
  <section class="debug">
    <h1 class="debug__title">诊断</h1>

    <p class="debug__lead">
      这一页把工作目录、数据库、当前偏好与仓库默认保护摊开。报告由内置的信息拼成，
      复制出去即可整段贴给别人看。
    </p>

    <div class="debug__actions">
      <button class="debug__btn debug__btn--primary" type="button" @click="copyFacts">
        <Copy :size="14" :stroke-width="2"/>
        复制全部
      </button>
    </div>

    <section v-for="section in sections" :key="section.title" class="debug__section">
      <h2 class="debug__heading">{{ section.title }}</h2>
      <dl class="debug__list">
        <div v-for="row in section.rows" :key="row.label" class="debug__row">
          <dt class="debug__label">{{ row.label }}</dt>
          <dd class="debug__value">{{ row.value }}</dd>
        </div>
      </dl>
    </section>
  </section>
</template>

<style scoped>
.debug {
  padding-top: 18px;
}

.debug__title {
  margin: 0;
  font-size: 1.7em;
}

.debug__lead {
  margin: 10px 0 0;
  color: var(--text-dim);
  font-size: 0.95em;
  line-height: 1.7;
}

.debug__actions {
  display: flex;
  align-items: center;
  gap: 8px;
  margin: 14px 0 0;
}

.debug__btn {
  appearance: none;
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 5px 12px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: var(--surface);
  color: var(--text);
  font: inherit;
  cursor: pointer;
}

.debug__btn:hover {
  background: var(--hover);
}

.debug__btn--primary {
  border-color: var(--accent);
  color: var(--accent);
}

.debug__section {
  margin-top: 26px;
}

.debug__heading {
  margin: 0 0 8px;
  color: var(--text-dim);
  font-size: 1.05em;
  font-weight: 600;
}

.debug__list {
  margin: 0;
  border: 1px solid var(--border);
  border-radius: 8px;
  overflow: hidden;
}

.debug__row {
  display: grid;
  grid-template-columns: minmax(120px, 0.32fr) 1fr;
  gap: 14px;
  padding: 8px 12px;
  /* 偶数行淡淡分一下，长报告才看得清哪一行配哪一行 */
  background: var(--surface);
}

.debug__row:nth-child(even) {
  background: var(--bg);
}

.debug__label {
  margin: 0;
  color: var(--text-dim);
  font-size: 0.92em;
}

.debug__value {
  margin: 0;
  font-family: var(--mono-font);
  font-size: 0.92em;
  /* 值里可能是路径或长串：允许换行，并让长串断开而不是撑破版心 */
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  user-select: text;
  -webkit-user-select: text;
}
</style>
