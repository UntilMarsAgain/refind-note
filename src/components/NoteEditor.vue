<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { Check, Pencil, Save, Trash2, X } from "@lucide/vue";
import { checkTitle } from "../title";
import { themeMode } from "../theme";
// `codemirror` 是元包（提供 basicSetup 等），EditorState 由 @codemirror/state 提供 ——
// 后者必须作为**直接依赖**安装：pnpm 的严格 node_modules 下，传递依赖不可直接导入。
import { basicSetup } from "codemirror";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { markdown } from "@codemirror/lang-markdown";
import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { tags } from "@lezer/highlight";
import type { DecorationSet, ViewUpdate } from "@codemirror/view";
import { Decoration, MatchDecorator, ViewPlugin } from "@codemirror/view";

/**
 * 顶用的编辑器：一个纯文本框 + 一排真按钮。
 *
 * 刻意不做所见即所得，也不做实时预览——编辑走的是代码编辑，
 * 以后换成 CodeMirror 时这一层的对外接口（保存草稿 / 提交 / 放弃 / 取消）不用动。
 *
 * 自动保存与「改了哪些」都由 App 持有（`modelValue`），这里只负责显示与派发；
 * 四个按钮都是真调用后端命令，没有一个占位。
 */

const props = defineProps<{
  /** 当前标题，用于改名与「有没有改过」的判断 */
  title: string;
  /** 编辑器里的源码，由 App 持有 */
  modelValue: string;
  /** 正在保存或提交时禁掉按钮，避免连点 */
  busy: boolean;
  /** 状态行：已保存草稿 / 提交冲突 / 失败原因 */
  status: string;
}>();

/**
 * CodeMirror 的配色**全部接项目的 token**，所以它跟着主题与主题色走，不必写两套样式。
 *
 * 为什么必须显式给：CM6 自带的 `basicSetup` 是按**浅色底**配的（正文近黑、高亮偏暗），
 * 铺在深色主题上就是"浅色面板 + 浅色文字"，看不清。这里做两件事：
 * 1. `EditorView.theme` 覆盖界面色（背景、正文、光标、选区、行号、当前行、提示框）；
 * 2. `syntaxHighlighting` 换掉默认高亮，改用 token 里的 `--syntax-*` 系列 ——
 *    与正文代码块同一套颜色。
 *
 * 值全是 `var(--…)`，所以切换深浅主题、甚至改主题色都不需要重新配置编辑器。
 */
const appTheme = EditorView.theme(
  {
    "&": { color: "var(--text)", backgroundColor: "transparent", height: "100%" },
    ".cm-content": { caretColor: "var(--accent)", fontFamily: "var(--mono-font)" },
    ".cm-cursor, .cm-dropCursor": { borderLeftColor: "var(--accent)" },
    "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection":
      { backgroundColor: "var(--selection-bg)" },
    ".cm-gutters": {
      backgroundColor: "transparent",
      color: "var(--text-dim)",
      border: "none",
    },
    ".cm-activeLine": { backgroundColor: "var(--hover)" },
    ".cm-activeLineGutter": { backgroundColor: "var(--hover)" },
    ".cm-panels": { backgroundColor: "var(--surface)", color: "var(--text)" },
    ".cm-tooltip": {
      backgroundColor: "var(--surface)",
      color: "var(--text)",
      border: "1px solid var(--border)",
    },
    ".cm-tooltip-autocomplete > ul > li[aria-selected]": {
      backgroundColor: "var(--hover)",
      color: "var(--text)",
    },
    /* 自定义的 [[内部链接]] 装饰（CM6 生成的元素不在 scoped 作用域里，只能写在这里） */
    ".cm-wikilink": {
      color: "var(--accent)",
      borderBottom: "1px dotted var(--accent)",
    },
  },
  {
    // 显式选深色就是深色；"跟随系统"才去问系统（写反过一次：显式 dark 会被系统偏好否掉）
    dark:
      themeMode.value === "dark" ||
      (themeMode.value === "system" &&
        !window.matchMedia("(prefers-color-scheme: light)").matches),
  },
);

const appHighlight = HighlightStyle.define([
  { tag: tags.heading, color: "var(--syntax-title)", fontWeight: "600" },
  { tag: tags.strong, color: "var(--text)", fontWeight: "600" },
  { tag: tags.emphasis, color: "var(--text)", fontStyle: "italic" },
  { tag: tags.link, color: "var(--accent)", textDecoration: "underline" },
  { tag: tags.url, color: "var(--accent)" },
  { tag: tags.monospace, color: "var(--syntax-string)" },
  { tag: tags.quote, color: "var(--syntax-comment)" },
  { tag: tags.list, color: "var(--syntax-number)" },
  { tag: tags.contentSeparator, color: "var(--border)" },
  { tag: tags.processingInstruction, color: "var(--syntax-keyword)" },
]);

/**
 * `[[内部链接]]` 的高亮。
 *
 * CM6 的 markdown 语法并不认识它（那是本项目的扩展语法），所以在**视图层**加装饰：
 * 只加样式、不改文档，保存下来的仍然是原文，渲染依旧由后端负责 —— 编辑器不做第二套解析。
 *
 * 匹配的是整个 `[[…]]`，所以里面无论写标题、`名称#章节` 还是 `名称@view-xxx`，
 * 都会被一起标出来（地址的识别本就在这一对方括号里）。
 */
const wikilinkMatcher = new MatchDecorator({
  regexp: /\[\[[^\]\n]+\]\]/g,
  decoration: Decoration.mark({ class: "cm-wikilink" }),
});

const wikilinkHighlight = ViewPlugin.fromClass(
  class {
    decorations: DecorationSet;

    constructor(view: EditorView) {
      this.decorations = wikilinkMatcher.createDeco(view);
    }

    update(update: ViewUpdate) {
      this.decorations = wikilinkMatcher.updateDeco(update, this.decorations);
    }
  },
  { decorations: (plugin) => plugin.decorations },
);

/** CodeMirror 挂载点 */
const hostEl = ref<HTMLElement | null>(null);
let view: EditorView | null = null;

/**
 * 右侧预览的 html。
 *
 * 由**后端**渲染（`render_markdown`），与阅读视图同一个渲染器 —— 所以预览里的
 * 表格、内部链接、代码高亮与正文逐字一致，不会出现"预览好看、提交后变样"。
 */
const preview = ref("");
let previewTimer: number | undefined;

async function refreshPreview(text: string) {
  try {
    preview.value = await invoke<string>("render_markdown", { markdown: text });
  } catch (error) {
    preview.value = "";
    previewProblem.value = String(error);
  }
}

/**
 * 预览**不追着输入跑**：连续 5 秒没有输入才渲染一次。
 *
 * 渲染要往返后端（而且是完整 markdown 渲染），逐字触发既费也可能打断思路；
 * 想看当前内容时按「刷新预览」立刻渲染。
 */
const PREVIEW_IDLE_MS = 5000;

function schedulePreview(text: string) {
  window.clearTimeout(previewTimer);
  previewTimer = window.setTimeout(() => void refreshPreview(text), PREVIEW_IDLE_MS);
}

/** 手动刷新预览（不等静默） */
function refreshPreviewNow() {
  window.clearTimeout(previewTimer);
  void refreshPreview(props.modelValue);
}

const previewProblem = ref("");

const emit = defineEmits<{
  (e: "update:modelValue", value: string): void;
  (e: "rename", title: string): void;
  (e: "save-draft"): void;
  (e: "commit", summary: string): void;
  (e: "discard"): void;
  (e: "cancel"): void;
}>();

/** 提交摘要，可留空 */
const summary = ref("");

/**
 * 标题输入框。
 *
 * 改名放在这里而不是标题栏：标题栏那一行是「跳转」语义（像浏览器地址栏），
 * 而改名是一次真实的提交，放在编辑场景里更不容易误触。
 */
const newTitle = ref(props.title);

onMounted(() => {
  if (!hostEl.value) {
    return;
  }

  view = new EditorView({
    parent: hostEl.value,
    state: EditorState.create({
      doc: props.modelValue,
      extensions: [
        basicSetup,
        markdown(),
        // 顺序有讲究：主题与高亮都要排在 basicSetup **之后**，才能盖掉它的浅色默认值
        appTheme,
        syntaxHighlighting(appHighlight),
        wikilinkHighlight,
        EditorView.lineWrapping,
        EditorView.updateListener.of((update) => {
          if (!update.docChanged) {
            return;
          }
          const text = update.state.doc.toString();
          emit("update:modelValue", text);
          schedulePreview(text);
        }),
      ],
    }),
  });

  void refreshPreview(props.modelValue);
});

onBeforeUnmount(() => {
  window.clearTimeout(previewTimer);
  view?.destroy();
  view = null;
});

// 外部换了内容（切换笔记、丢弃草稿、提交后回填）时把编辑器同步过去。
// 判等是必需的：否则每个按键都会把内容重设一遍，光标会被打回开头。
watch(
  () => props.modelValue,
  (value) => {
    if (view && value !== view.state.doc.toString()) {
      view.dispatch({
        changes: { from: 0, to: view.state.doc.length, insert: value },
      });
    }
    schedulePreview(value);
  },
);

// 改名成功或切换笔记后，输入框要跟上新的标题
watch(
  () => props.title,
  (value) => {
    newTitle.value = value;
  },
);

/** 词法问题（空、@、非法字符、过长）即时反馈，不打扰后端 */
const renameProblem = ref<string | null>(null);

function localCheck() {
  renameProblem.value = newTitle.value === props.title ? null : checkTitle(newTitle.value);
  return renameProblem.value;
}

function submitRename() {
  const value = newTitle.value.trim();
  if (!value || value === props.title) {
    return;
  }
  if (localCheck()) {
    return;
  }

  // 词法之外还有后端才知道的规则（命名空间前缀），所以落盘前问一次权威判定
  void invoke("validate_title", { title: value })
    .then(() => emit("rename", value))
    .catch((error) => {
      renameProblem.value = String(error);
    });
}

function onInput(event: Event) {
  const target = event.target;
  if (target instanceof HTMLTextAreaElement) {
    emit("update:modelValue", target.value);
  }
}

function submit() {
  emit("commit", summary.value.trim());
}
</script>

<template>
  <section class="editor">
    <div class="editor__names">
      <input
        v-model="newTitle"
        class="editor__name"
        type="text"
        aria-label="笔记标题"
        @keydown.enter.prevent="submitRename"
      />
      <button
        v-if="newTitle.trim() && newTitle.trim() !== title"
        class="ebtn"
        type="button"
        :disabled="busy || localCheck() !== null"
        @click="submitRename"
      >
        <Pencil :size="14" :stroke-width="1.9" />
        改名为「{{ newTitle.trim() }}」
      </button>
      <span v-if="renameProblem" class="editor__problem">{{ renameProblem }}</span>
    </div>

    <div class="editor__bar">

      <input
        v-model="summary"
        class="editor__summary"
        type="text"
        placeholder="提交摘要（可留空）"
        @keydown.enter.prevent="submit"
      />

      <div class="editor__actions">
        <button
          class="editor__preview-btn"
          type="button"
          title="不等静默，立刻渲染当前内容"
          @click="refreshPreviewNow"
        >
          刷新预览
        </button>

        <button class="ebtn" type="button" :disabled="busy" @click="emit('save-draft')">
          <Save :size="14" :stroke-width="1.9" />
          保存草稿
        </button>
        <button
          class="ebtn ebtn--primary"
          type="button"
          :disabled="busy"
          @click="submit"
        >
          <Check :size="14" :stroke-width="2.2" />
          提交
        </button>
        <button
          class="ebtn ebtn--danger"
          type="button"
          :disabled="busy"
          @click="emit('discard')"
        >
          <Trash2 :size="14" :stroke-width="1.9" />
          放弃草稿
        </button>
        <button class="ebtn" type="button" :disabled="busy" @click="emit('cancel')">
          <X :size="14" :stroke-width="1.9" />
          取消
        </button>
      </div>
    </div>

    <!--
      分栏直接写在元素上。样式表层面这两条本来也是并排（后出现的规则是 flex row），
      写成内联是为了排除"被某条更靠后的规则覆盖"这一可能 —— 内联样式只有 !important 能压。
    -->
    <div
      class="editor__panes"
      style="display: flex; flex-direction: row; align-items: stretch; gap: 12px"
    >
      <!-- 左：源码（CodeMirror） -->
      <div
        ref="hostEl"
        class="editor__source selectable"
        style="flex: 1 1 0; min-width: 0"
      />

      <!-- 右：渲染预览（后端同一个渲染器；.note-body 复用正文样式） -->
      <div
        class="editor__preview selectable"
        style="flex: 1 1 0; min-width: 0"
      >
        <p v-if="previewProblem" class="editor__preview-error">
          预览渲染失败：{{ previewProblem }}
        </p>
        <div v-else class="note-body" v-html="preview" />
      </div>
    </div>

    <textarea
      v-if="false"
      class="editor__text selectable"
      :value="modelValue"
      spellcheck="false"
      autocapitalize="off"
      autocorrect="off"
      @input="onInput"
    />

    <p class="editor__status">
      <span class="editor__message">{{ status }}</span>
      <span>{{ modelValue.length }} 字符</span>
    </p>
  </section>
</template>

<style scoped>
/* 根撑满可用高度：父容器（编辑页时）不再滚动，高度从它一路传下来 */
.editor {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
  padding-top: 18px;
}

.editor__names {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  align-items: center;
  margin-bottom: 8px;
}

.editor__problem {
  color: var(--link-missing);
  font-size: 12.5px;
}

.editor__name {
  flex: 1 1 260px;
  min-width: 0;
  height: 30px;
  padding: 0 10px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: var(--field-bg);
  color: var(--text);
  font: inherit;
  font-size: 14px;
  font-weight: 600;
}

.editor__bar {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
  align-items: center;
  margin-bottom: 10px;
}

.editor__summary {
  flex: 1 1 200px;
  min-width: 0;
  height: 30px;
  padding: 0 10px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: var(--field-bg);
  color: var(--text);
  font: inherit;
  font-size: 13px;
}

.editor__summary::placeholder {
  color: var(--text-dim);
}

.editor__actions {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}

.ebtn {
  appearance: none;
  display: inline-flex;
  align-items: center;
  gap: 5px;
  height: 30px;
  padding: 0 11px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: transparent;
  color: var(--text-dim);
  font-size: 13px;
  line-height: 1.4;
  cursor: pointer;
  transition: background-color 120ms ease, color 120ms ease,
    border-color 120ms ease;
}

.ebtn:hover:not(:disabled) {
  background: var(--hover);
  color: var(--text);
}

.ebtn:disabled {
  opacity: 0.5;
  cursor: default;
}

.ebtn--primary {
  border-color: var(--accent-soft);
  color: var(--accent-soft);
}

.ebtn--primary:hover:not(:disabled) {
  background: var(--accent);
  color: var(--text);
}

.ebtn--danger:hover:not(:disabled) {
  border-color: var(--link-missing);
  color: var(--link-missing);
}

.editor__text {
  /* 暂用大 min-height 顶住；换成 CodeMirror 时这里改由它接管 */
  min-height: 58vh;
  padding: 12px 14px;
  border: 1px solid var(--border);
  border-radius: 8px;
  background: var(--field-bg);
  color: var(--text);
  font-family: var(--mono-font);
  font-size: 13.5px;
  line-height: 1.7;
  resize: vertical;
  tab-size: 2;
}

.editor__status {
  display: flex;
  justify-content: space-between;
  gap: 12px;
  margin: 8px 0 0;
  color: var(--text-dim);
  font-size: 12.5px;
}

.editor__message:empty::before {
  /* ---------- 源码 / 预览 两栏 ---------- */

.editor__panes {
  display: grid;
  flex: 1 1 auto;
  grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
  gap: 12px;
  min-height: 320px;
}

.editor__source,
.editor__preview {
  min-width: 0;
  border: 1px solid var(--border);
  border-radius: 8px;
  overflow: auto;
}

.editor__source {
  background: var(--field-bg);
}

.editor__preview {
  padding: 0 14px;
  background: var(--surface);
}

/*
 * CodeMirror 撑满左栏。
 *
 * `.cm-*` 是它自己用 JS 插进来的元素，**不带本组件的 scoped 属性**，所以普通后代选择器
 * 选不到（之前那两条其实一直没生效）。要穿透作用域，必须用 :deep()。
 */
.editor__source :deep(.cm-editor) {
  height: 100%;
}

.editor__source :deep(.cm-scroller) {
  font-family: var(--mono-font);
  font-size: 13px;
  line-height: 1.7;
}

.editor__preview-error {
  color: var(--link-missing);
  font-size: 13px;
}

/* 预览里不出复制符号与行内代码的手型（它是预览，不是正文） */
.editor__preview .note-body a[href]::after {
  display: none;
}

/*
 * 左源码 / 右预览：**先把宽度对半分给两栏，再让各栏在自己的栏内排版**。
 *
 * 这里用 flex 而不是 grid，是为了把"先分栏"这件事写死：
 * `flex: 1 1 0` 让两栏各占一半（基准是 0，不是内容宽度）；
 * `min-width: 0` 才允许它们被压到半屏以下 —— 少了它，宽表格或长代码行的**固有宽度**
 * 会把栏顶开，布局就退化成上下排列（这正是"自己去占据空间"）。
 */
.editor__panes {
  display: flex;
  flex-direction: row;
  width: 100%;
  align-items: stretch;
  gap: 12px;
}

.editor__source,
.editor__preview {
  flex: 1 1 0;
  min-width: 0;
}


/*
 * 刷新预览。选择器带上 `.editor__actions`：
 * 同组按钮的通用样式若写在文件更靠后的位置，仅凭类名会被它盖住（上一版就是因此
 * 显示成浅色默认按钮）。这里用更高特异性，确保颜色与边框由本规则说了算。
 */
.editor__actions .editor__preview-btn {
  padding: 5px 10px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: transparent;
  color: var(--text-dim);
  font-size: 12px;
  cursor: pointer;
}

.editor__actions .editor__preview-btn:hover {
  background: var(--hover);
  color: var(--text);
}

/*
 * 两栏**各自滚动**，而不是整页滚动。
 *
 * 关键不是加 overflow，而是**高度要有确定来源**：高度由内容撑开时，页面本身就会变高，
 * 滚轮滚的就是页面。所以这里走 flex 链 —— 根 `.editor` 撑满父容器，两栏吃掉剩余高度，
 * 再各自 overflow: auto。
 *
 * 下面那条 `:global(...)` 是降级路径：父容器不支持 `:has()` 时按视口高度兜底
 * （旧行为：两栏各有滚动条，页面可能还能滚一点）。
 */
.editor__panes {
  flex: 1 1 auto;
  height: auto;
  min-height: 0;
}

/* 没有 :has() 的老实现环境：父容器仍在滚，只能按视口给一个固定高度兜底 */
:global(.app__body:not(:has(.editor))) .editor__panes {
  height: calc(100vh - 190px);
  min-height: 320px;
}

/* 状态为空时也占住这一行，避免布局上下跳 */
  content: "　";
}
</style>
