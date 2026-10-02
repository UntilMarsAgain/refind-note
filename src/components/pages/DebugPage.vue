<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { Copy } from "@lucide/vue";
import { invoke } from "@tauri-apps/api/core";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { THEME_LABELS } from "../../ipc/settings.ts";
import type { Diagnostics } from "../../ipc/diagnostics.ts";
import { flash } from "../../core/notice.ts";
import { isMobile } from "../../core/platform.ts";
import { syncReady } from "../../core/sync.ts";
import { FILE_SCHEME } from "../../dom/file-links.ts";
import {
  databaseMeta,
  databaseRoot,
  gpgAvailable,
  preferences,
  protection,
  workspaceRoot,
} from "../../core/preferences.ts";

/**
 * 诊断页（`special:debug`）：数据位置、当前偏好，以及**只有后端才知道**的那几件事
 * （这台设备是什么、各条路径落在哪、同步配成什么样、仓库里有多少东西、
 * 渲染那一层带着哪些语法与模板）。
 *
 * 报告分成几段摊开：段是"哪一块"，行是"哪一项"。**显示与整段复制用的是同一份**，
 * 否则"看到的"和"抄走的"迟早对不上。
 *
 * 前半段（偏好、数据库）界面上本来就有，直接读；后半段走 `diagnostics` 命令。
 * 命令失败不影响前半段 —— 诊断页自己打不开就没用了。
 */

/** 没打开时统一用这个占位 */
const NOT_OPEN = "（没打开）";

const root = computed(() => workspaceRoot.value || NOT_OPEN);
const dbDir = computed(() => databaseRoot.value || NOT_OPEN);
const preferencesFile = computed(() =>
  workspaceRoot.value ? `${workspaceRoot.value}/settings/preferences.json` : NOT_OPEN,
);
const configFile = computed(() =>
  workspaceRoot.value ? `${workspaceRoot.value}/settings/repository.json` : NOT_OPEN,
);

function yesNo(value: boolean): string {
  return value ? "是" : "否";
}

/** 字节数说成人话（诊断页看的是量级，不是精确值） */
function bytes(count: number): string {
  if (count < 1024) {
    return `${count} B`;
  }
  const units = ["KB", "MB", "GB", "TB"];
  let value = count / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(1)} ${units[unit]}`;
}

/** 后端给的那一半（只有后端知道：设备、路径、同步、仓库规模、渲染器、系统集成） */
const facts = ref<Diagnostics | null>(null);
const factsProblem = ref("");

onMounted(async () => {
  try {
    facts.value = await invoke<Diagnostics>("diagnostics");
  } catch (error) {
    factsProblem.value = String(error);
  }
  await checkEngines();
});

/**
 * 前端这一层的自检：公式与画图的引擎**真的能不能加载**。
 *
 * 它们是**动态 import** 的（笔记里没有公式/图就不加载），所以"这台机器上能不能用"
 * 只有真去 import 一次才知道 —— 产物少了某个 chunk、移动端取不到那一份，
 * 症状都是"公式/图不出现"，而别的都正常。这里主动试一次，把结果写下来。
 */
const engines = ref([
  { label: "公式引擎（KaTeX）", value: "正在试…" },
  { label: "画图引擎（mermaid）", value: "正在试…" },
]);

async function checkEngines() {
  const katex = await trial(async () => {
    const module = await import("katex");
    return `能加载（${module.default.version}）`;
  });
  const mermaid = await trial(async () => {
    const module = await import("mermaid");
    const version = (module.default as { version?: string }).version;
    return version ? `能加载（${version}）` : "能加载";
  });
  engines.value = [
    { label: "公式引擎（KaTeX）", value: katex },
    { label: "画图引擎（mermaid）", value: mermaid },
  ];
}

async function trial(run: () => Promise<string>): Promise<string> {
  try {
    return await run();
  } catch (error) {
    return `加载不了：${error}`;
  }
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
    ...extraSections(),
  ];
});

/**
 * 后端那几段。
 *
 * 还没读回来（或者读失败）就只放一行说明 —— 段的数量会变，
 * 但**看得见的与复制走的始终是同一份**。
 */
function extraSections(): { title: string; rows: { label: string; value: string }[] }[] {
  const data = facts.value;
  if (!data) {
    return [
      {
        title: "后端",
        rows: [
          {
            label: "读取",
            value: factsProblem.value ? `失败：${factsProblem.value}` : "正在读…",
          },
        ],
      },
    ];
  }

  const sync = data.sync;
  const repo = data.repository;

  return [
    {
      title: "这台设备",
      rows: [
        { label: "类型", value: data.platform === "mobile" ? "手机（mobile）" : "桌面（desktop）" },
        { label: "界面认为", value: isMobile() ? "手机" : "桌面" },
        { label: "仓库根目录", value: data.workspace_root },
        { label: "临时文件", value: data.scratch_dir },
        { label: "导出落点", value: data.download_dir },
        { label: "文件地址头", value: FILE_SCHEME },
      ],
    },
    {
      title: "云端同步",
      rows: [
        { label: "可用", value: yesNo(sync.ready) },
        { label: "界面那颗按钮", value: syncReady() ? "已显示" : "未显示" },
        { label: "服务地址", value: sync.settings.endpoint || "（没填）" },
        {
          label: "桶 / 前缀",
          value: `${sync.settings.bucket || "（没填）"} / ${sync.settings.prefix || "（无）"}`,
        },
        { label: "云端加密", value: sync.settings.has_key ? "有密钥" : "没有密钥（传的是明文）" },
        { label: "加密算法", value: sync.settings.cipher },
        { label: "S3 私钥", value: sync.settings.has_secret ? "已设置" : "没设置" },
        { label: "待整份重传", value: yesNo(sync.reupload_pending) },
        {
          label: "记账本",
          value: `${sync.index_files} 份${sync.index_updated ? `（记于 ${sync.index_updated}）` : ""}`,
        },
      ],
    },
    {
      title: "仓库",
      rows: [
        { label: "事件日志", value: `${repo.logs} 份` },
        { label: "内容块", value: `${repo.blobs} 个，共 ${bytes(repo.blob_bytes)}` },
        { label: "草稿槽位", value: `${repo.drafts} 个` },
        { label: "回收站", value: `${repo.trash} 条` },
        { label: "数据库占用", value: bytes(repo.database_bytes) },
      ],
    },
    {
      title: "渲染",
      rows: [
        { label: "自定义语法", value: data.renderer.syntax.join("、") },
        {
          label: "内置模板",
          value: `${data.renderer.templates.length} 个：${data.renderer.templates.join("、")}`,
        },
        { label: "帮助页", value: data.renderer.help_pages.join("、") },
        ...engines.value,
      ],
    },
    {
      title: "系统集成",
      rows: [
        { label: "refind:// 注册", value: data.system.deep_link },
        { label: "gpg 版本", value: data.system.gpg_version || "（没有 gpg）" },
        { label: "WebView", value: webviewName() },
      ],
    },
  ];
}

/** WebView 是哪一个（从 UA 里认；认不出就把前半段原文抄上） */
function webviewName(): string {
  const agent = navigator.userAgent;
  for (const [name, marker] of [
    ["WebKitGTK（Linux）", "WebKit"],
    ["WebView2（Windows）", "Edg/"],
    ["WKWebView（macOS / iOS）", "Safari"],
    ["Android WebView", "Android"],
  ] as const) {
    if (agent.includes(marker)) {
      return `${name} —— ${agent.slice(0, 80)}`;
    }
  }
  return agent.slice(0, 80);
}

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
      本页汇总数据位置、当前偏好、云端同步、仓库规模与渲染能力等诊断信息，
      可整段复制以便排查问题。</br>
      这里也报只有后端才知道的那几件事（这台设备是什么、各条路径落在哪、
      同步配成什么样、公式与画图的引擎能不能加载）—— 出问题时先看这一页。
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
