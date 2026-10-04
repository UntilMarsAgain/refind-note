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
import { computed, onBeforeUnmount, onDeactivated, onMounted, shallowRef, watch } from "vue";
import type { EditorView } from "@codemirror/view";
import { Check, ImagePlus, RotateCcw, Save, Trash2, X } from "@lucide/vue";
import StoragePicker from "../common/StoragePicker.vue";
import EditorPanes from "./EditorPanes.vue";
import { useAttachmentInsert } from "../../composables/useAttachmentInsert.ts";
import { useCodeMirror } from "../../composables/useCodeMirror.ts";
import { useNoteEditing } from "../../composables/useNoteEditing.ts";
import { useNotePreview } from "../../composables/useNotePreview.ts";
import { protection } from "../../core/preferences.ts";
import { countOf, describeCount } from "../../core/count.ts";

/**
 * 笔记编辑器。
 *
 * 左栏是 CodeMirror 6 的源码视图，右栏是后端渲染出来的预览；上面一排真按钮：
 * 保存草稿、提交、放弃草稿、取消。源码由这个组件自己持有 —— 装载笔记、恢复草稿、
 * 自动保存都基于它。
 *
 * 具体的活都拆出去了，这个文件只剩**接线**：谁的状态交给谁、什么时候装载、
 * 内容变了往哪推。各自为什么这么写，见各自那一段：
 *
 * - `useNoteEditing` —— 源码、草稿槽位、提交与丢弃、自动保存的节流；
 * - `useCodeMirror` —— 那个编辑器实例的挂载、销毁与内容同步；
 * - `useNotePreview` —— 预览什么时候去渲染（不追着输入跑，见 `PREVIEW_IDLE_MS`）；
 * - `useAttachmentInsert` —— 选文件 / 粘贴图片 → 上传 → 在光标处插入引用；
 * - `EditorPanes` —— 那两栏的摆放（并排还是上下），它自己量宽度。
 *
 * 「提交完成」与「取消」都要换地址，地址归上层管，所以这里只把**要去的地址**派发出去。
 */
const props = defineProps<{
    /** 正在编辑的笔记标题 */
    title: string;
    /**
     * 这个编辑器属于哪个标签页。
     *
     * 有了它，切走再切回来时**正在写的字、光标与滚动都在原地**（见 `core/editor-state.ts`）；
     * 不给也不影响用，只是没有这一层"原位还原"。
     */
    tabId?: string;
}>();

const emit = defineEmits<{
    /** 离开编辑器（提交完成或取消）后要去的地址：这篇笔记的阅读地址 */
    (e: "navigate", title: string): void;
}>();

/**
 * CodeMirror 实例就放在这一个 ref 上，两边共用。
 *
 * 编辑状态要靠它读光标与滚动（切走再切回来原位还原），编辑器靠它挂载与销毁；
 * 共用一个 ref 才不会出现"记的是一份、拆的是另一份"。
 */
const view = shallowRef<EditorView | null>(null);

const editor = useNoteEditing({
    title: () => props.title,
    tabId: () => props.tabId,
    // 装载与恢复光标都发生在编辑器建起来之前，那时它还是 null
    view,
    navigate: (title) => emit("navigate", title),
});

const attachments = useAttachmentInsert({
    view,
    markdown: editor.markdown,
    status: editor.status,
    busy: editor.busy,
    // 附件进的是同一个 blob 仓，它也可能要口令；而这里没有单独的口令栏 ——
    // 就借这一版笔记刚输过的那把。要不要口令看的是**仓库默认**
    // （`editor.perCommit` 那一栏管的是这篇笔记怎么存，与附件无关）
    passphrase: editor.passphraseDraft,
    defaultPolicy: protection,
});

/**
 * 状态行右边那几个数：写了多少。
 *
 * 规则在 `core/count.ts`（中文一字一算、拉丁连着算一个，字符按**码点**数 ——
 * `text.length` 会把一个 emoji 数成两个）。这里只负责算出来摆上去。
 */
const counted = computed(() => countOf(editor.markdown.value));

const codeMirror = useCodeMirror({
    view,
    text: () => editor.markdown.value,
    onDocChanged: (text) => {
        editor.markdown.value = text;
        editor.recordEditing();
        // 输入后延迟自动保存，连续敲字不会每次都写
        editor.scheduleAutosave();
    },
    onSelectionChanged: () => editor.recordEditing(),
    onPaste: (event) => attachments.onPasteFiles(event),
});

const preview = useNotePreview(
    () => props.title,
    () => editor.markdown.value,
);

/** 退出编辑但**保留**草稿：回到这篇的阅读地址 */
function leave() {
    emit("navigate", props.title);
}

onMounted(async () => {
    await editor.load();
    // 装载失败（或上了锁）就不建编辑器：那两栏没什么可编辑的
    if (!editor.hasNote.value) {
        return;
    }
    codeMirror.mount();
    // 编辑器建起来了，把装载时欠下的那次光标/滚动还原补上。
    // 这一步不能省：load() 必须跑在 mount() 之前（初始文档就是装载的结果），
    // 所以装载那一刻要还原的位子只能先记着 —— 少了这一次，"切回这个标签页时
    // 视线落回原处"就只在"重试"那条路上生效。
    editor.applyPendingRestore();
    void preview.refresh(editor.markdown.value);
});

/**
 * 切走时（标签页被 KeepAlive 缓存起来）：**编辑器还在**，所以这一次落盘是实打实的。
 *
 * 这一条是常驻带来的：以前切标签页会卸载组件，`onBeforeUnmount` 顺手把草稿兜底
 * 落一次；现在不卸载了，那次落盘就没有了 —— 只剩"停手三秒"的自动保存顶着。
 * 开着编辑器切走、立刻关窗，那三秒里的字就悬在半空。所以这里补上。
 *
 * `codeMirror.destroy()` **不在这里做**：那是拆实例，切走时实例还要留着（切回来
 * 继续用同一个编辑器）。
 */
onDeactivated(() => {
    editor.flushOnLeave();
});

onBeforeUnmount(() => {
    // 顺序要紧：先把状态记下、草稿落盘（那时编辑器还在，光标才读得到），
    // 再拆编辑器。各自模块里的定时器与观察器由它们自己的 onBeforeUnmount 钩子清。
    editor.flushOnLeave();
    codeMirror.destroy();
});

// 内容变了就同步编辑器与预览
watch(editor.markdown, (value) => {
    codeMirror.syncText(value);
    // 预览则重新排队渲染
    preview.schedule(value);
});
</script>

<template>
  <section class="editor">
    <header class="editor__head">
      <h1 class="editor__title">{{ title }}</h1>
    </header>

    <div class="editor__bar">
      <input
          v-model="editor.summary.value"
          class="editor__summary"
          type="text"
          placeholder="提交说明（可留空）"
          @keydown.enter.prevent="editor.commit"
      />

      <div class="editor__actions">
        <StoragePicker
            v-model:policy="editor.perCommit.value"
            v-model:passphrase="editor.passphraseDraft.value"
        />

        <button
            class="ebtn"
            type="button"
            title="上传文件，并在光标处插入引用（也可以直接 Ctrl+V 粘贴）"
            :disabled="editor.busy.value || !editor.hasNote.value"
            @click="attachments.insertFile"
        >
          <ImagePlus :size="14" :stroke-width="1.9"/>
          插入文件
        </button>

        <button
            class="ebtn"
            type="button"
            title="不等静默，立刻渲染当前内容"
            @click="preview.refreshNow"
        >
          刷新预览
        </button>

        <button
            class="ebtn"
            type="button"
            :disabled="editor.busy.value || !editor.hasNote.value"
            @click="editor.saveDraft"
        >
          <Save :size="14" :stroke-width="1.9"/>
          保存草稿
        </button>
        <button
            class="ebtn ebtn--primary"
            type="button"
            :disabled="editor.busy.value || !editor.hasNote.value"
            @click="editor.commit"
        >
          <Check :size="14" :stroke-width="2.2"/>
          提交
        </button>
        <button
            class="ebtn ebtn--danger"
            type="button"
            :disabled="editor.busy.value || !editor.hasDraft.value"
            @click="editor.discard"
        >
          <Trash2 :size="14" :stroke-width="1.9"/>
          放弃草稿
        </button>
        <button class="ebtn" type="button" :disabled="editor.busy.value" @click="leave">
          <X :size="14" :stroke-width="1.9"/>
          取消
        </button>
      </div>
    </div>

    <!-- 有未提交的草稿：是否继续编辑由此处决定，此期间不覆盖草稿 -->
    <div v-if="editor.pendingDraft.value" class="editor__draft">
      <span class="editor__draft-text">
        存在一份未提交的草稿（{{ editor.pendingDraft.value.modified }}）。
        当前显示的是已提交的版本。
      </span>
      <button
          class="ebtn"
          type="button"
          :disabled="editor.busy.value || editor.loading.value"
          @click="editor.restoreDraft"
      >
        <RotateCcw :size="14" :stroke-width="1.9"/>
        恢复草稿
      </button>
      <button
          class="ebtn"
          type="button"
          :disabled="editor.busy.value || editor.loading.value"
          @click="editor.discardPending"
      >
        丢弃草稿
      </button>
    </div>

    <p v-if="editor.loading.value" class="editor__problem">正在读「{{ title }}」…</p>
    <div v-else-if="editor.loadProblem.value" class="editor__problem">
      <p>{{ editor.loadProblem.value }}</p>

      <!-- 读不出来（例如解锁被取消）也要有出路：重试，或者退回阅读页 -->
      <div v-if="!editor.locked.value" class="editor__problem-actions">
        <button type="button" class="ebtn" @click="editor.load">重试</button>
        <button type="button" class="ebtn" @click="emit('navigate', props.title)">返回阅读页</button>
      </div>

      <div v-if="editor.locked.value" class="editor__problem-actions">
        <button
            type="button"
            class="ebtn"
            @click="emit('navigate', `${props.title}@unlock`)"
        >
          去解锁
        </button>
        <button type="button" class="ebtn" @click="editor.writeAnyway">直接写新的一版</button>
        <!-- 口令想不起来时，这几件事都不用先解开这一版 -->
        <button
            type="button"
            class="ebtn"
            @click="emit('navigate', `${props.title}@history`)"
        >
          版本历史
        </button>
        <button
            type="button"
            class="ebtn ebtn--danger"
            @click="emit('navigate', `${props.title}@delete`)"
        >
          删除这一篇
        </button>
      </div>
    </div>

    <!-- 两栏：源码 / 预览。摆放与测量都在那个组件里 -->
    <EditorPanes
        :title="props.title"
        :preview="preview.preview.value"
        :preview-problem="preview.previewProblem.value"
        :host-el="codeMirror.hostEl"
    />

    <p class="editor__status">
      <span class="editor__message">{{ editor.status.value }}</span>
      <!-- 写了多少：状态行右侧。行数也摆上，翻长稿时有用 -->
      <span class="editor__meta">
        <span>{{ describeCount(counted) }}</span>
        <span>{{ counted.lines }} 行</span>
      </span>
    </p>
  </section>
</template>

<style scoped>
.editor {
  display: flex;
  flex-direction: column;
  min-height: 0;
  padding-top: 18px;
}

.editor__head {
  padding: 8px 0;
}

.editor__title {
  margin: 0;
  font-size: 26px;
  font-weight: 600;
  line-height: 1.35;
  overflow-wrap: anywhere;
}

.editor__problem {
  margin: 0 0 8px;
  color: var(--link-missing);
  font-size: 12.5px;
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
  background: var(--link-missing);
  color: var(--link-missing);
}

.editor__status {
  display: flex;
  justify-content: space-between;
  gap: 12px;
  margin: 8px 0 0;
  color: var(--text-dim);
  font-size: 12.5px;
}

.editor__meta {
  display: flex;
  align-items: center;
  gap: 8px;
}

/* 状态为空时也占住这一行，避免布局上下跳 */
.editor__message:empty::before {
  content: "　";
}

.editor__draft {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  align-items: center;
  margin: 0 0 10px;
  padding: 8px 12px;
  border-left: 3px solid var(--accent-soft);
  border-radius: 6px;
  background: var(--accent-tint);
  color: var(--text-dim);
  font-size: 12.5px;
}

.editor__draft-text {
  flex: 1 1 240px;
  min-width: 0;
}

/* 打不开时给两条路：去解锁，或者不看旧内容直接写新的一版 */
.editor__problem-actions {
  display: flex;
  gap: 8px;
  margin-top: 10px;
}
</style>