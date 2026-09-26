<script setup lang="ts">
/**
 * 诊断（`special:debug`）：把"仓库现在是什么样"摊开成可读的分段。
 *
 * 它是 `special:` 下的一项普通系统页面（与回收站、数据库回收同类），不是调试残留：
 * 排查"模板取不到""页面去哪了""占了多少空间"这类问题时，这里是唯一能一次看全的地方。
 *
 * 报告**全部由后端生成**，这一层只显示与复制；唯一的例外是"界面实测"那一段 ——
 * 窗口宽度、版心宽度这种东西后端看不到，只能前端补。
 */
import { onBeforeUnmount, onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import ViaHint from "./ViaHint.vue";
import type { DebugEntry, DebugReport, Via } from "../bindings";

const props = defineProps<{
  /** 当前地址：报告里会多一段"当前页" */
  address: string;
  /** 是被哪条指令带到这一页的（null = 直接打开） */
  via: Via | null;
}>();

const emit = defineEmits<{ (e: "open-via", title: string): void }>();

const report = ref<DebugReport | null>(null);
const layout = ref<DebugEntry[]>([]);
const problem = ref("");
const busy = ref(false);
const notice = ref("");

/**
 * 界面自身的实测值。
 *
 * 之所以要这一段：宽度类问题（"全屏了还是不够宽"）的答案往往就是"版心比窗口窄多少"，
 * 而后端看不到窗口与版心。
 */
function measureLayout(): DebugEntry[] {
  const column = document.querySelector(".app__column") as HTMLElement | null;
  return [
    { label: "窗口宽度（CSS px）", value: String(window.innerWidth) },
    { label: "窗口高度（CSS px）", value: String(window.innerHeight) },
    { label: "像素比", value: String(window.devicePixelRatio) },
    {
      label: "版心宽度",
      value: column ? column.clientWidth + " px" : "（当前没有版心）",
    },
    {
      label: "标签栏占用",
      value:
        String(window.innerWidth - (column ? column.clientWidth : 0)) +
        " px（含内边距）",
    },
  ];
}

async function refresh() {
  busy.value = true;
  problem.value = "";
  notice.value = "";
  layout.value = measureLayout();
  try {
    report.value = await invoke<DebugReport>("debug_report", {
      address: props.address,
    });
  } catch (error) {
    problem.value = String(error);
  } finally {
    busy.value = false;
  }
}

/** 摊平成纯文本，方便整段贴到别处 */
function flatten(): string {
  const lines: string[] = [];
  const push = (title: string, entries: DebugEntry[]) => {
    lines.push("【" + title + "】");
    for (const entry of entries) {
      lines.push("  " + entry.label + "：" + entry.value);
    }
    lines.push("");
  };
  push("界面实测", layout.value);
  for (const section of report.value?.sections ?? []) {
    push(section.title, section.entries);
  }
  return lines.join("\n");
}

async function copyAll() {
  if (!report.value) {
    await refresh();
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

/**
 * 定位那一行（后端报告里的路径之类太长，点一下就滚到它并选中）。
 * 这里只滚 + 高亮，不做别的：诊断页不该有副作用。
 */
function onResize() {
  layout.value = measureLayout();
}

onMounted(() => {
  void refresh();
  window.addEventListener("resize", onResize);
});

onBeforeUnmount(() => {
  window.removeEventListener("resize", onResize);
});
</script>

<template>
  <section class="debug">
    <h1 class="debug__title">诊断</h1>
    <ViaHint :via="via" @open-via="emit('open-via', $event)" />

    <p class="debug__lead">
      这一页把仓库当前的状态摊开：版本与环境、设置、命名空间、内容体积、当前页的解析与渲染。
      报告由后端生成，复制出去即可整段贴给别人看。
    </p>

    <div class="debug__actions">
      <button class="debug__btn" type="button" :disabled="busy" @click="refresh">
        {{ busy ? "正在收集…" : "重新收集" }}
      </button>
      <button class="debug__btn debug__btn--primary" type="button" @click="copyAll">
        复制全部
      </button>
      <span v-if="notice" class="debug__notice">{{ notice }}</span>
    </div>

    <p v-if="problem" class="debug__problem">{{ problem }}</p>

    <section class="debug__section">
      <h2 class="debug__heading">界面实测</h2>
      <dl class="debug__list">
        <div v-for="entry in layout" :key="entry.label" class="debug__row">
          <dt class="debug__label">{{ entry.label }}</dt>
          <dd class="debug__value">{{ entry.value }}</dd>
        </div>
      </dl>
    </section>

    <section
      v-for="section in report?.sections ?? []"
      :key="section.title"
      class="debug__section"
    >
      <h2 class="debug__heading">{{ section.title }}</h2>
      <dl class="debug__list">
        <div v-for="entry in section.entries" :key="entry.label" class="debug__row">
          <dt class="debug__label">{{ entry.label }}</dt>
          <dd class="debug__value">{{ entry.value }}</dd>
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

.debug__btn:disabled {
  opacity: 0.6;
  cursor: default;
}

.debug__btn--primary {
  border-color: var(--accent);
  color: var(--accent);
}

.debug__notice {
  color: var(--text-dim);
  font-size: 0.9em;
}

.debug__problem {
  margin: 12px 0 0;
  padding: 8px 12px;
  border-left: 3px solid var(--border);
  color: var(--text-dim);
  font-size: 0.92em;
}

.debug__section {
  margin-top: 26px;
}

.debug__heading {
  margin: 0 0 8px;
  font-size: 1.05em;
  color: var(--text-dim);
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
  /* 值里可能有路径与 HTML：允许换行，并让长串断开而不是撑破版心 */
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  user-select: text;
  -webkit-user-select: text;
}
</style>
