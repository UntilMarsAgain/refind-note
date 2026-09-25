<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { Check, Pencil, Save, Trash2, X } from "@lucide/vue";
import { checkTitle } from "../title";
// `codemirror` 是元包（提供 basicSetup 等），EditorState 由 @codemirror/state 提供 ——
// 后者必须作为**直接依赖**安装：pnpm 的严格 node_modules 下，传递依赖不可直接导入。
import { basicSetup } from "codemirror";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { markdown } from "@codemirror/lang-markdown";

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

/** 输入频繁，预览节流一下（180ms）：不必每敲一个字都往返一次后端 */
function schedulePreview(text: string) {
  window.clearTimeout(previewTimer);
  previewTimer = window.setTimeout(() => void refreshPreview(text), 180);
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

    <div class="editor__panes">
      <!-- 左：源码（CodeMirror） -->
      <div ref="hostEl" class="editor__source selectable" />

      <!-- 右：渲染预览（后端同一个渲染器；.note-body 复用正文样式） -->
      <div class="editor__preview selectable">
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
.editor {
  display: flex;
  flex-direction: column;
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

/* CodeMirror 撑满左栏 */
.editor__source .cm-editor {
  height: 100%;
}

.editor__source .cm-scroller {
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

/* 状态为空时也占住这一行，避免布局上下跳 */
  content: "　";
}
</style>
