<script setup lang="ts">
import { onMounted, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import FloatingTools from "./components/FloatingTools.vue";
import HistoryView from "./components/HistoryView.vue";
import MissingNote from "./components/MissingNote.vue";
import NoteContent from "./components/NoteContent.vue";
import NoteEditor from "./components/NoteEditor.vue";
import NoteSearch from "./components/NoteSearch.vue";
import PageHeader from "./components/PageHeader.vue";
import TabRail from "./components/TabRail.vue";
import TitleBar from "./components/TitleBar.vue";
import WindowResizeHandles from "./components/WindowResizeHandles.vue";
import { PREFERENCE_KEYS, readFlag, writeFlag } from "./settings";

/** 与 Rust 端 `NoteSummary` 对应（标签栏用，不含正文） */
interface NoteSummary {
  key: string;
  title: string;
  rev: number;
  modified: string;
}

/** 与 Rust 端 `Note` 对应 */
interface Note {
  key: string;
  title: string;
  markdown: string;
  html: string;
  rev: number;
  modified: string;
}

/** 与 Rust 端 `LoadOutcome` 对应。`note` 为 null 表示目标还不存在。 */
interface LoadOutcome {
  note: Note | null;
  title: string;
  deleted: boolean;
}

/** 与 Rust 端 `Draft` 对应 */
interface Draft {
  markdown: string;
  base_rev: number;
  at: string;
}

/** 与 Rust 端 `VaultSettings` 对应 */
interface VaultSettings {
  root: string;
  format: number;
  capital_links: boolean;
  max_title_bytes: number;
}

/** 与 Rust 端 `RevisionContent` 对应（历史里某一版的正文） */
interface RevisionContent {
  rev: number;
  kind: string;
  at: string;
  title: string;
  markdown: string;
  html: string;
}

type Mode = "read" | "edit" | "missing" | "history";

/** 自动保存：停手三秒后写一条草稿到链上 */
const AUTOSAVE_DELAY_MS = 3000;

const notes = ref<NoteSummary[]>([]);
const note = ref<Note | null>(null);
const mode = ref<Mode>("read");
/** 「不存在」视图里请求的标题（创建按钮用它） */
const missingTitle = ref("");
const loadError = ref("");
const vaultRoot = ref("");
/** 正文滚下去之后，页头收起并贴顶冻结 */
const scrolled = ref(false);
/** 滚动容器，滚动到底/回顶都作用在它上面 */
const scrollEl = ref<HTMLElement | null>(null);
/** 正文是否限制为阅读栏宽度 */
const limitWidth = ref(readFlag(PREFERENCE_KEYS.limitWidth, true));

// 编辑器：源码由这里持有，自动保存与提交都基于它
const draftText = ref("");
/** 保存 / 提交进行中，用来禁掉按钮避免连点 */
const busy = ref(false);
const editorStatus = ref("");
/** 搜索面板是否打开 */
const searchOpen = ref(false);
let autosaveTimer: number | undefined;

async function refreshNotes() {
  notes.value = await invoke<NoteSummary[]>("list_notes");
}

/** 切到「不存在」视图。红链与地址栏输入新标题都会走到这里。 */
function showMissing(title: string) {
  note.value = null;
  missingTitle.value = title;
  mode.value = "missing";
}

/**
 * 打开一篇笔记。
 *
 * `load_note` 把「目标不存在」当成正常结果返回（而不是错误），因为那需要界面配合：
 * 显示「还没有这篇笔记」和一个创建按钮。
 */
async function openNote(title: string) {
  try {
    const outcome = await invoke<LoadOutcome>("load_note", { title });
    if (!outcome.note) {
      showMissing(outcome.title);
      return;
    }

    note.value = outcome.note;
    missingTitle.value = "";
    mode.value = "read";
    loadError.value = "";
    scrolled.value = false;
    scrollEl.value?.scrollTo({ top: 0 });
  } catch (error) {
    console.debug("打开笔记失败:", title, error);
    // 只有在还没有任何笔记可看时，才让错误占满正文
    if (!note.value) {
      loadError.value = String(error);
    }
  }
}

/** 建立一篇笔记，然后直接进编辑器（空白笔记没有什么可读的） */
async function createNote(title: string) {
  const trimmed = title.trim();
  if (!trimmed) {
    return;
  }

  busy.value = true;
  try {
    const created = await invoke<Note>("create_note", { title: trimmed });
    note.value = created;
    missingTitle.value = "";
    loadError.value = "";
    await refreshNotes();
    await beginEditing();
  } catch (error) {
    loadError.value = String(error);
  } finally {
    busy.value = false;
  }
}

async function beginEditing() {
  const current = note.value;
  if (!current) {
    return;
  }

  draftText.value = current.markdown;
  editorStatus.value = "";
  mode.value = "edit";
  scrolled.value = false;

  // 上次没提交的草稿要恢复出来，否则自动保存就白做了
  try {
    const draft = await invoke<Draft | null>("load_draft", { title: current.title });
    if (draft && draft.markdown !== current.markdown) {
      draftText.value = draft.markdown;
      editorStatus.value = `已恢复未提交的草稿（${draft.at}）`;
    }
  } catch (error) {
    console.debug("读取草稿失败:", error);
  }
}

async function saveDraft() {
  const current = note.value;
  if (!current || mode.value !== "edit") {
    return;
  }
  // 没改动就不追加草稿节点（后端也会幂等挡住，这里省一次往返）
  if (draftText.value === current.markdown) {
    return;
  }

  try {
    // 注意参数名：Tauri v2 默认把命令参数转成 camelCase，base_rev 要写成 baseRev
    await invoke("save_draft", {
      title: current.title,
      markdown: draftText.value,
      baseRev: current.rev,
    });
    editorStatus.value = `已自动保存为草稿（${new Date().toLocaleTimeString()}）`;
  } catch (error) {
    editorStatus.value = `草稿保存失败：${String(error)}`;
  }
}

async function commitEditing(summary: string) {
  const current = note.value;
  if (!current) {
    return;
  }

  busy.value = true;
  try {
    const committed = await invoke<Note>("commit_note", {
      title: current.title,
      markdown: draftText.value,
      summary: summary || null,
      baseRev: current.rev,
    });

    note.value = committed;
    mode.value = "read";
    editorStatus.value = "";
    await refreshNotes();
    scrolled.value = false;
    scrollEl.value?.scrollTo({ top: 0 });
  } catch (error) {
    // 提交冲突会走到这里：不动用户正在编辑的内容，只把原因写在状态行
    editorStatus.value = `提交失败：${String(error)}`;
  } finally {
    busy.value = false;
  }
}

/** 放弃草稿：把草稿节点从日志里删掉，回到上一次提交的内容 */
async function discardDraft() {
  const current = note.value;
  if (!current) {
    return;
  }

  busy.value = true;
  try {
    await invoke("discard_draft", { title: current.title });
    draftText.value = current.markdown;
    editorStatus.value = "草稿已丢弃，已回到上一次提交的内容";
  } catch (error) {
    editorStatus.value = `丢弃失败：${String(error)}`;
  } finally {
    busy.value = false;
  }
}

/** 退出编辑但**保留**草稿（与「放弃草稿」区分开） */
function leaveEditor() {
  mode.value = "read";
  editorStatus.value = "";
}

function openHistory() {
  mode.value = "history";
}

function closeHistory() {
  mode.value = "read";
}

/**
 * 把历史里某一版取回编辑器。
 *
 * 刻意**不改写旧历史**：取回的内容先成为当前草稿，提交后是一个新版本，
 * 于是「回退」也只是版本链上正常的一步。
 */
function restoreVersion(content: RevisionContent) {
  draftText.value = content.markdown;
  editorStatus.value = `已取回版本 ${content.rev} 的内容；提交后会成为一个新版本`;
  mode.value = "edit";
}

/**
 * 改名。
 *
 * 注意它本身是一次提交（内容不变、标题变），版本号会往前一格——编辑器之后的
 * 自动保存与提交必须用新的 `base_rev`，否则会撞上冲突守卫。
 */
async function renameCurrent(nextTitle: string) {
  const current = note.value;
  if (!current) {
    return;
  }

  busy.value = true;
  try {
    const renamed = await invoke<Note>("rename_note", {
      from: current.title,
      to: nextTitle,
    });
    note.value = renamed;
    await refreshNotes();
    editorStatus.value = `已改名为「${renamed.title}」（改名本身记为一版）`;
  } catch (error) {
    editorStatus.value = `改名失败：${String(error)}`;
  } finally {
    busy.value = false;
  }
}

// 输入后延迟自动保存，连续敲字不会每次都写
watch(draftText, () => {
  if (mode.value !== "edit") {
    return;
  }
  window.clearTimeout(autosaveTimer);
  autosaveTimer = window.setTimeout(() => void saveDraft(), AUTOSAVE_DELAY_MS);
});

onMounted(async () => {
  try {
    const settings = await invoke<VaultSettings>("get_settings");
    vaultRoot.value = settings.root;
  } catch (error) {
    console.debug("读取仓库设置失败:", error);
  }

  try {
    await refreshNotes();
    const first = notes.value[0];
    if (first) {
      await openNote(first.title);
    } else {
      loadError.value = "仓库里还没有笔记";
    }
  } catch (error) {
    loadError.value = String(error);
  }
});

/**
 * 收起页头。
 *
 * 收起会让页头矮约 20px，若笔记恰好只比视口高一点点，就会出现
 * 「收起 -> 内容变矮 -> scrollTop 被夹回 -> 展开」的临界抖动。
 * 所以展开比收起更早触发，留出回差。
 */
function onScroll(event: Event) {
  const el = event.currentTarget;
  if (!(el instanceof HTMLElement)) {
    return;
  }

  const { scrollTop } = el;
  if (!scrolled.value && scrollTop > 32) {
    scrolled.value = true;
  } else if (scrolled.value && scrollTop < 12) {
    scrolled.value = false;
  }
}

function toggleWidth() {
  limitWidth.value = !limitWidth.value;
  writeFlag(PREFERENCE_KEYS.limitWidth, limitWidth.value);
}

function scrollToTop() {
  scrollEl.value?.scrollTo({ top: 0, behavior: "smooth" });
}

function scrollToBottom() {
  const el = scrollEl.value;
  if (el) {
    el.scrollTo({ top: el.scrollHeight, behavior: "smooth" });
  }
}

function onNoteSelected(title: string) {
  // 切走之前先把草稿落盘：自动保存有三秒延迟，直接切会丢掉刚敲的字。
  // saveDraft 在第一个 await 之前就把 note.value 读定了，所以不会误存到新打开的笔记上。
  if (mode.value === "edit") {
    void saveDraft();
  }
  void openNote(title);
}

/** 标签栏里的「新建笔记」：取一个没被占用的名字再建 */
function createNoteFromRail() {
  const taken = new Set(notes.value.map((item) => item.title));
  let title = "未命名";
  for (let index = 2; taken.has(title); index += 1) {
    title = `未命名 ${index}`;
  }
  void createNote(title);
}

function onSearch() {
  searchOpen.value = true;
}

function onMenu() {
  // TODO: 展开菜单（展开内容之后再接）
}

/** 地址栏提交 = 按标题打开（目标不存在会切到「不存在」视图） */
function onSubmit(value: string) {
  void openNote(value);
}

/**
 * 内部链接。
 *
 * 存在与否是后端渲染时判定的（`data-missing`）：红链直接切到「不存在」视图，
 * 省一次往返；蓝链才真去读。
 */
function onWikiLink(payload: { title: string; missing: boolean }) {
  if (payload.missing) {
    showMissing(payload.title);
    return;
  }
  void openNote(payload.title);
}

function onAction(name: string) {
  if (name === "edit") {
    void beginEditing();
    return;
  }
  if (name === "history") {
    openHistory();
    return;
  }
  console.debug("page action:", name);
}
</script>

<template>
  <WindowResizeHandles />

  <NoteSearch
    :notes="notes"
    :open="searchOpen"
    @close="searchOpen = false"
    @open-note="onNoteSelected"
  />

  <div class="app">
    <TitleBar
      :title="note?.title ?? missingTitle"
      @search="onSearch"
      @menu="onMenu"
      @submit="onSubmit"
    />

    <div class="app__main">
      <TabRail
        :notes="notes"
        :active="note?.key ?? ''"
        @open="onNoteSelected"
        @create="createNoteFromRail"
      />

      <main
        ref="scrollEl"
        class="app__body"
        :class="{ 'app__body--wide': !limitWidth }"
        @scroll.passive="onScroll"
      >
        <div class="app__column" :class="{ 'app__column--wide': !limitWidth }">
          <!-- 编辑中：不显示页头，操作都在编辑器自己那一行里 -->
          <NoteEditor
            v-if="mode === 'edit' && note"
            v-model="draftText"
            :title="note.title"
            :busy="busy"
            :status="editorStatus"
            @rename="renameCurrent"
            @save-draft="saveDraft"
            @commit="commitEditing"
            @discard="discardDraft"
            @cancel="leaveEditor"
          />

          <MissingNote
            v-else-if="mode === 'missing'"
            :title="missingTitle"
            @create="createNote(missingTitle)"
          />

          <HistoryView
            v-else-if="mode === 'history' && note"
            :title="note.title"
            :current-rev="note.rev"
            @close="closeHistory"
            @restore="restoreVersion"
          />

          <template v-else-if="note">
            <PageHeader
              :title="note.title"
              :collapsed="scrolled"
              @action="onAction"
            />
            <NoteContent :html="note.html" @wikilink="onWikiLink" />
          </template>

          <p v-else-if="loadError" class="app__error">{{ loadError }}</p>

          <p v-else class="app__empty">仓库位置：{{ vaultRoot || "（未能读取）" }}</p>
        </div>
      </main>
    </div>
  </div>

  <FloatingTools
    :limited="limitWidth"
    @toggle-width="toggleWidth"
    @scroll-top="scrollToTop"
    @scroll-bottom="scrollToBottom"
  />
</template>

<style scoped>
.app {
  display: flex;
  flex-direction: column;
  height: 100vh;
}

/* min-height: 0 是必须的：否则这个 flex 项会被内容撑开，
   里面 .app__body 的 overflow 就再也滚不动了 */
.app__main {
  display: flex;
  flex: 1 1 auto;
  min-height: 0;
}

.app__body {
  flex: 1 1 auto;
  /* 同理，允许它被 TabRail 挤窄 */
  min-width: 0;
  overflow: auto;
  /* 顶部留白交给 PageHeader，这样它贴顶冻结时不会有缝 */
  padding: 0 32px 64px;
  transition: padding 200ms ease;
}

/* 取消限宽时给右下角那组悬浮按钮让出位置，
   否则正文会跑到它们底下被挡住 */
.app__body--wide {
  padding-right: 64px;
}

/* 阅读栏：页头与正文共用同一条，限宽并居中 */
.app__column {
  max-width: var(--reading-width);
  margin-inline: auto;
  /* 切换限宽时让版心平滑变化，而不是硬跳 */
  transition: max-width 200ms ease;
}

/* 取消限宽：两侧只留容器自己的少量内边距。
   用 100% 而不是 none —— none 不可插值，上面的过渡会直接断掉。 */
.app__column--wide {
  max-width: 100%;
}

.app__error {
  margin: 28px 0 0;
  color: var(--accent-soft);
  font-size: 14px;
}

.app__empty {
  margin: 28px 0 0;
  color: var(--text-dim);
  font-size: 13.5px;
}

@media (prefers-reduced-motion: reduce) {
  .app__body,
  .app__column {
    transition: none;
  }
}
</style>
