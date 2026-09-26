<script setup lang="ts">
/**
 * 「状态」面板：把编辑页当前的事实摊开。
 *
 * 分两半：**前端测得到的**（编辑状态、布局实测）与**只有后端才知道的**
 * （这次渲染的规模、耗时、模板块走向）。后者按需收集（点「收集」），不跟着每次输入跑 ——
 * 预览本身走另一条命令，面板不该拖慢打字。
 *
 * 单独一个组件而不是塞进编辑器：编辑器里 scoped 样式不生效（历史包袱），这里可以正常写；
 * 顺带它也能用在别处。
 */
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import type { Draft, RenderReport } from "../bindings";
import { isBlank, templateMarks } from "../template-blocks";

const props = defineProps<{
  /** 当前标题 */
  title: string;
  /** 当前地址（后端据此判定语言） */
  address: string;
  /** 编辑器里的文本 */
  markdown: string;
  /** 后端判定出的语言（null = markdown） */
  language: string | null;
  /** 编辑器的状态行文案（已保存草稿 / 提交冲突 / 失败原因） */
  status: string;
  /** 光标位置（CM6 的状态，只有编辑器自己知道） */
  cursor: string;
  /** 选区（同上） */
  selection: string;
}>();

const report = ref<RenderReport | null>(null);
const busy = ref(false);
const problem = ref("");
const notice = ref("");
const layout = ref<[string, string][]>([]);

/**
 * 模板块：编辑器**实际**会怎么标。
 *
 * 用的是编辑器同一个纯函数，所以这里报的就是屏幕上该出现的东西 ——
 * 于是"屏幕与这里不一致"与"这里本身就错"能分开看。
 *
 * 空白一律显示成 `·`：这一类问题十有八九藏在看不见的空格里
 * （上一轮那条孤零零的色块，就是一行"带空白的空行"）。
 */
const blockRows = computed<[string, string][]>(() => {
  const lines = props.markdown.split("\n");
  const first = new Map<number, { head: boolean; start: number; end: number }>();
  for (const mark of templateMarks(lines)) {
    first.set(mark.line, mark);
  }
  return lines.map((text, index) => {
    const mark = first.get(index);
    const kind = mark
      ? mark.head
        ? "头行"
        : "块内"
      : isBlank(text)
        ? "空行（不标）"
        : "—";
    const range = mark ? mark.start + "–" + mark.end : "—";
    const shown = text.replace(/ /g, "·").replace(/\t/g, "→");
    return ["第 " + index + " 行 ｜ " + kind + " ｜ 列 " + range, shown === "" ? "（空）" : shown];
  });
});

/** DOM 里到底有没有这些类、样式算出来是什么 —— 区分"规则错"与"样式没生效" */
const markDom = ref("（未测）");

function measureMarks() {
  const heads = document.querySelectorAll(".cm-template-head").length;
  const bodies = document.querySelectorAll(".cm-template-body").length;
  const sample = document.querySelector(".cm-template-body");
  const style = sample ? getComputedStyle(sample) : null;
  markDom.value =
    ".cm-template-head " + heads + " 个、.cm-template-body " + bodies + " 个" +
    (style ? "；首个块内标记底色 " + style.backgroundColor : "；没有块内标记");
}
/** 草稿的落盘时间只有后端知道：收一次，顺带把"存过没有"说清 */
const draftInfo = ref("（未收集）");

async function loadDraft() {
  try {
    const draft = await invoke<Draft | null>("load_draft", { title: props.title });
    if (!draft) {
      draftInfo.value = "没有（与已提交的一致，或还没存过）";
      return;
    }
    const at = new Date(draft.at);
    const shown = Number.isNaN(at.getTime()) ? draft.at : at.toLocaleString();
    draftInfo.value = "有，落盘于 " + shown + "（基于版本 " + draft.base_rev + "）";
  } catch (error) {
    draftInfo.value = "读不出来：" + String(error);
  }
}

/**
 * 布局实测。
 *
 * 宽度类问题的答案往往就是"版心比窗口窄多少""两栏各多宽"，
 * 而这些数只有前端看得到 —— 后端没有窗口。
 */
function measureLayout(): [string, string][] {
  const column = document.querySelector(".app__column") as HTMLElement | null;
  const panes = document.querySelector(".editor__panes") as HTMLElement | null;
  const rows: [string, string][] = [
    ["窗口", window.innerWidth + " × " + window.innerHeight + "（CSS px）"],
    ["像素比", String(window.devicePixelRatio)],
    ["版心宽度", column ? column.clientWidth + " px" : "（没有版心）"],
  ];
  if (panes) {
    const direction = getComputedStyle(panes).flexDirection;
    rows.push(["两栏容器", panes.clientWidth + " px，方向 " + direction]);
    Array.from(panes.children).forEach((child, index) => {
      const element = child as HTMLElement;
      rows.push([
        "　第 " + (index + 1) + " 栏",
        element.clientWidth + " px",
      ]);
    });
  }
  return rows;
}

function refreshLayout() {
  layout.value = measureLayout();
  measureMarks();
}

async function collect() {
  busy.value = true;
  problem.value = "";
  notice.value = "";
  refreshLayout();
  void loadDraft();
  try {
    report.value = await invoke<RenderReport>("render_report", {
      markdown: props.markdown,
      address: props.address,
    });
  } catch (error) {
    problem.value = String(error);
  } finally {
    busy.value = false;
  }
}

/** 摊平成纯文本：整段贴给别人看时用得上 */
function flatten(): string {
  const lines: string[] = ["【编辑状态】"];
  for (const [label, value] of editRows.value) {
    lines.push("  " + label + "：" + value);
  }
  lines.push("");
  if (report.value) {
    lines.push("【预览编译】");
    lines.push("  输入：" + report.value.markdown_bytes + " 字节 / " + report.value.markdown_lines + " 行");
    lines.push("  输出：" + report.value.html_bytes + " 字节 HTML");
    lines.push("  耗时：" + report.value.millis + " ms");
    lines.push("  语言：" + report.value.language);
    for (const entry of report.value.blocks) {
      lines.push("  " + entry.label + " → " + entry.value);
    }
    lines.push("");
    lines.push("【渲染出的 HTML】");
    lines.push(report.value.html);
    lines.push("");
  }
  lines.push("【模板块标记】");
  lines.push("  " + markDom.value);
  for (const [label, value] of blockRows.value) {
    lines.push("  " + label + "：" + value);
  }
  lines.push("");
  lines.push("【模板块标记】");
  lines.push("  " + markDom.value);
  for (const [label, value] of blockRows.value) {
    lines.push("  " + label + "：" + value);
  }
  lines.push("");
  lines.push("【布局实测】");
  for (const [label, value] of layout.value) {
    lines.push("  " + label + "：" + value);
  }
  return lines.join("\n");
}

async function copyAll() {
  if (!report.value) {
    await collect();
  }
  try {
    await writeText(flatten());
    notice.value = "已复制";
  } catch (error) {
    problem.value = "复制失败：" + String(error);
  }
  window.setTimeout(() => {
    notice.value = "";
  }, 1500);
}

/** 编辑状态：都是前端手里现成的事实 */
const editRows = computed<[string, string][]>(() => [
  ["标题", props.title],
  ["地址", props.address],
  ["语言", props.language ?? "markdown"],
  ["规模", props.markdown.length + " 字符 / " + props.markdown.split("\n").length + " 行"],
  ["光标", props.cursor],
  ["选区", props.selection],
  // 草稿是后端的账，收一次才知道（见 loadDraft）
  ["草稿", draftInfo.value],
  ["状态行", props.status || "（无）"],
]);

onMounted(() => {
  refreshLayout();
  void loadDraft();
  window.addEventListener("resize", refreshLayout);
});

onBeforeUnmount(() => {
  window.removeEventListener("resize", refreshLayout);
});
</script>

<template>
  <details class="state">
    <summary class="state__summary">
      状态
      <span class="state__digest">
        {{ markdown.length }} 字符 · {{ language ?? "markdown" }} ·
        {{ status || "无异常" }}
      </span>
    </summary>

    <div class="state__actions">
      <button class="state__btn" type="button" :disabled="busy" @click="collect">
        {{ busy ? "正在收集…" : "收集编译信息" }}
      </button>
      <button class="state__btn state__btn--primary" type="button" @click="copyAll">
        复制全部
      </button>
      <span v-if="notice" class="state__notice">{{ notice }}</span>
    </div>

    <p v-if="problem" class="state__problem">{{ problem }}</p>

    <table class="state__table">
      <caption>编辑状态</caption>
      <tbody>
        <tr v-for="[label, value] in editRows" :key="label">
          <th>{{ label }}</th>
          <td>{{ value }}</td>
        </tr>
      </tbody>
    </table>

    <table v-if="report" class="state__table">
      <caption>预览编译</caption>
      <tbody>
        <tr>
          <th>输入</th>
          <td>{{ report.markdown_bytes }} 字节 / {{ report.markdown_lines }} 行</td>
        </tr>
        <tr>
          <th>输出</th>
          <td>{{ report.html_bytes }} 字节 HTML</td>
        </tr>
        <tr>
          <th>耗时</th>
          <td>{{ report.millis }} ms</td>
        </tr>
        <tr>
          <th>语言</th>
          <td>{{ report.language }}</td>
        </tr>
        <tr v-for="entry in report.blocks" :key="entry.label">
          <th class="state__mono">{{ entry.label }}</th>
          <td>{{ entry.value }}</td>
        </tr>
      </tbody>
    </table>

    <details v-if="report" class="state__html">
      <summary>渲染出的 HTML（{{ report.html_bytes }} 字节）</summary>
      <pre>{{ report.html }}</pre>
    </details>

    <table class="state__table">
      <caption>模板块（编辑器实际会怎么标）</caption>
      <tbody>
        <tr>
          <th>DOM 里的标记</th>
          <td>{{ markDom }}</td>
        </tr>
        <tr v-for="[label, value] in blockRows" :key="label">
          <th class="state__mono">{{ label }}</th>
          <td class="state__mono">{{ value }}</td>
        </tr>
      </tbody>
    </table>

    <table class="state__table">
      <caption>模板块（编辑器实际会怎么标）</caption>
      <tbody>
        <tr>
          <th>DOM 里的标记</th>
          <td>{{ markDom }}</td>
        </tr>
        <tr v-for="[label, value] in blockRows" :key="label">
          <th class="state__mono">{{ label }}</th>
          <td class="state__mono">{{ value }}</td>
        </tr>
      </tbody>
    </table>

    <table class="state__table">
      <caption>布局实测</caption>
      <tbody>
        <tr v-for="[label, value] in layout" :key="label">
          <th>{{ label }}</th>
          <td>{{ value }}</td>
        </tr>
      </tbody>
    </table>
  </details>
</template>

<!--
  非 scoped：编辑页那个组件里 scoped 样式不生效（历史包袱），
  而这个面板必须可靠地长成表格 —— 所以类名统一加 `state-` 前缀，避免影响别处。
-->
<style>
.state {
  margin-top: 14px;
  color: var(--text-dim);
  font-size: 12.5px;
}

.state__summary {
  cursor: pointer;
  user-select: none;
}

.state__digest {
  margin-left: 8px;
  color: var(--text-dim);
}

.state__actions {
  display: flex;
  align-items: center;
  gap: 8px;
  margin: 10px 0;
}

.state__btn {
  padding: 4px 10px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: var(--surface);
  color: var(--text);
  font: inherit;
  cursor: pointer;
}

.state__btn:hover {
  background: var(--hover);
}

.state__btn:disabled {
  opacity: 0.6;
  cursor: default;
}

.state__btn--primary {
  border-color: var(--accent);
  color: var(--accent);
}

.state__notice {
  color: var(--text-dim);
}

.state__problem {
  margin: 8px 0;
  padding: 6px 10px;
  border-left: 3px solid var(--border);
}

.state__table {
  width: 100%;
  margin: 10px 0 0;
  border-collapse: collapse;
  table-layout: fixed;
}

.state__table caption {
  padding-bottom: 4px;
  text-align: left;
  color: var(--text-dim);
}

.state__table th,
.state__table td {
  padding: 5px 8px;
  border: 1px solid var(--border);
  text-align: left;
  vertical-align: top;
  /* 长串（路径、HTML）要能断行，否则会把表格撑破 */
  overflow-wrap: anywhere;
  white-space: pre-wrap;
}

.state__table th {
  width: 34%;
  font-weight: 400;
  color: var(--text-dim);
  background: var(--surface);
}

.state__table td {
  /* 诊断内容要能复制出去 */
  user-select: text;
  -webkit-user-select: text;
}

.state__mono {
  font-family: var(--mono-font);
}

.state__html pre {
  margin: 8px 0 0;
  padding: 10px 12px;
  max-height: 260px;
  overflow: auto;
  border: 1px solid var(--border);
  border-radius: 8px;
  background: var(--surface);
  color: var(--text);
  font-size: 12px;
  line-height: 1.5;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  user-select: text;
  -webkit-user-select: text;
}
</style>
