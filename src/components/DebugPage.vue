<script setup lang="ts">
import { computed } from "vue";
import { Copy } from "@lucide/vue";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import type { ThemeMode } from "../bindings/settings.ts";
import { flash } from "../notice.ts";
import {
  databaseMeta,
  databaseRoot,
  gpgAvailable,
  preferences,
  protection,
  workspaceRoot,
} from "../preferences.ts";

/**
 * 诊断页（`special:debug`）：工作目录、数据库与当前偏好的现状。
 */

/** 没打开时统一用这个占位 */
const NOT_OPEN = "（没打开）";

const root = computed(() => workspaceRoot.value || NOT_OPEN);
const dbDir = computed(() => databaseRoot.value || NOT_OPEN);
const preferencesFile = computed(() =>
  workspaceRoot.value ? `${workspaceRoot.value}/settings/preferences.json` : NOT_OPEN,
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

/**
 * 一份事实，按分组摆好。
 *
 * **渲染与整段复制用的是同一份** —— 否则"看到的"和"抄走的"迟早对不上。
 */
const sections = computed(() => {
  const meta = databaseMeta.value;
  const prefs = preferences.value;
  const policy = protection.value;

  return [
    {
      title: "工作目录",
      rows: [{ label: "位置", value: root.value }],
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
        { label: "文件", value: preferencesFile.value },
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
  <section class="diag">
    <div class="diag__head">
      <h1 class="diag__title">诊断</h1>
      <button
        class="diag__copy"
        type="button"
        title="整段复制诊断信息"
        aria-label="整段复制"
        @click="copyFacts"
      >
        <Copy :size="14" :stroke-width="2" />
        <span>整段复制</span>
      </button>
    </div>

    <p class="diag__where">工作目录、数据库与当前偏好的现状</p>

    <template v-for="section in sections" :key="section.title">
      <h2 class="diag__section">{{ section.title }}</h2>
      <div v-for="row in section.rows" :key="row.label" class="row">
        <span class="row__label">{{ row.label }}</span>
        <code class="row__value">{{ row.value }}</code>
      </div>
    </template>
  </section>
</template>

<style scoped>
.diag {
  padding: 28px 0 64px;
}

.diag__head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 6px;
}

.diag__title {
  margin: 0;
  font-size: 22px;
}

.diag__copy {
  appearance: none;
  display: inline-flex;
  align-items: center;
  gap: 6px;
  height: 28px;
  padding: 0 12px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: transparent;
  color: var(--text-dim);
  font-size: 13px;
  cursor: pointer;
}

.diag__copy:hover {
  border-color: var(--accent-soft);
  background: var(--hover);
  color: var(--text);
}

.diag__where {
  margin: 0 0 24px;
  color: var(--text-dim);
  font-size: 13px;
}

.diag__section {
  margin: 26px 0 12px;
  padding-bottom: 6px;
  border-bottom: 1px solid var(--border);
  color: var(--text-dim);
  font-size: 14px;
  font-weight: 500;
}

.row {
  display: flex;
  align-items: baseline;
  /* 窄窗口下让"标签 + 值"换行，而不是被裁掉 */
  flex-wrap: wrap;
  gap: 12px;
  margin: 10px 0;
}

.row__label {
  min-width: 132px;
  font-size: 13px;
}

.row__value {
  padding: 1px 6px;
  border-radius: 4px;
  background: var(--hover);
  color: var(--text);
  font-family: var(--mono-font);
  font-size: 12px;
  overflow-wrap: anywhere;
  /* 这些路径是要抄下来贴进别处的 */
  -webkit-user-select: text;
  user-select: text;
}
</style>
