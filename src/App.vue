<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
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
  /** 草稿也是链上的一版，所以也有 commit ID —— 预览走的就是它 */
  id: string;
  short_id: string;
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

/** 与 Rust 端 `Address` 对应：地址栏那一行的解析结果（后端解析到底） */
type Address =
  | { kind: "empty" }
  | { kind: "note"; title: string; address: string }
  | {
      kind: "revision";
      title: string;
      rev: number;
      id: string;
      short_id: string;
      address: string;
    }
  | { kind: "edit"; title: string; address: string }
  | { kind: "history"; title: string; address: string }
  | { kind: "missing"; title: string; address: string };

/** 自动保存：停手三秒后写一条草稿到链上 */
const AUTOSAVE_DELAY_MS = 3000;

const notes = ref<NoteSummary[]>([]);
const note = ref<Note | null>(null);

/**
 * 当前地址的解析结果 —— **界面状态的权威副本**。
 *
 * 别的 ref（note / revisionView / missingTitle / draftExists …）都只是它派生出来的
 * 缓存，而且**只有 navigate() 会写它们**。「界面现在在哪、是什么状态」只有一份真相：
 * 后端对地址的解析结果。想改变它，唯一办法就是改地址。
 */
const route = ref<Address | null>(null);

/** 模式由权威副本推导 —— 自己不持有状态，就不可能和地址说法不一致 */
const mode = computed<Mode>(() => {
  switch (route.value?.kind) {
    case "edit":
      return "edit";
    case "history":
      return "history";
    case "missing":
      return "missing";
    default:
      return "read";
  }
});
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
/** 阅读页只显示最新提交；存在草稿时提示，由用户点开 */
const draftExists = ref(false);
/** 删除确认条 */
const confirmDelete = ref(false);
/** 草稿提醒被忽略过一次（换地址会重新出现） */
const draftHintDismissed = ref(false);
/** 删除时顺手回收悬置数据 */
const deleteWithGc = ref(false);
/** 地址栏解析出的错误（比如缩写写错了）；以前这类错误被悄悄吞掉了 */
const addressError = ref("");
let autosaveTimer: number | undefined;

async function refreshNotes() {
  notes.value = await invoke<NoteSummary[]>("list_notes");
}

/** 「不存在」视图的装载：模式由权威副本推导，这里只管标题 */
function showMissing(title: string) {
  note.value = null;
  missingTitle.value = title;
}

/**
 * 装载一篇笔记的内容。
 *
 * `load_note` 把「目标不存在」当成正常结果返回（而不是错误），因为那需要界面配合。
 * 但走到这里时地址解析已经确认它存在，所以正常不会缺内容；万一缺了就退回「不存在」
 * 视图 —— 仍然不改模式，模式只认权威副本。
 */
async function loadNote(title: string) {
  try {
    const outcome = await invoke<LoadOutcome>("load_note", { title });
    if (!outcome.note) {
      showMissing(outcome.title);
      return;
    }

    note.value = outcome.note;
    missingTitle.value = "";
    loadError.value = "";
    scrolled.value = false;
    scrollEl.value?.scrollTo({ top: 0 });
    void refreshDraftHint(outcome.note.title);
  } catch (error) {
    console.debug("装载笔记失败:", title, error);
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
    await invoke<Note>("create_note", { title: trimmed });
    loadError.value = "";
    await refreshNotes();
    // 建完进编辑器：同样通过改地址（`$edit`）
    await navigate(`${trimmed}$edit`);
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

    editorStatus.value = "";
    await refreshNotes();
    scrolled.value = false;
    scrollEl.value?.scrollTo({ top: 0 });
    // 提交之后回到「阅读最新提交」这个地址 —— 状态变化同样只走地址
    await navigate(committed.title);
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

/** 退出编辑但**保留**草稿（与「放弃草稿」区分开）：回到阅读地址 */
function leaveEditor() {
  editorStatus.value = "";
  const title = note.value?.title;
  if (title) {
    void navigate(title);
  }
}

/** 看版本历史：同样是改地址（`$history`） */
function openHistory() {
  const title = note.value?.title;
  if (title) {
    void navigate(`${title}$history`);
  }
}

function closeHistory() {
  const title = note.value?.title;
  if (title) {
    void navigate(title);
  }
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
    await refreshNotes();

    // 改名改变了「在哪」：地址必须跟着变。但编辑器里的内容不能丢 ——
    // 先把缓冲区作为草稿存到新标题上，再导航过去（navigate 会把草稿恢复出来）。
    if (draftText.value !== renamed.markdown) {
      await invoke("save_draft", {
        title: renamed.title,
        markdown: draftText.value,
        baseRev: renamed.rev,
      });
    }
    await navigate(`${renamed.title}$edit`);
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
      await navigate(first.title);
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

/** 历史页要求导航到某一版：拼成「标题@缩写」交给统一的地址解析 */
function onOpenRevision(reference: string) {
  const title = note.value?.title;
  if (title) {
    void navigate(`${title}@${reference}`);
  }
}

function onNoteSelected(title: string) {
  // 切走之前先把草稿落盘：自动保存有三秒延迟，直接切会丢掉刚敲的字。
  // saveDraft 在第一个 await 之前就把 note.value 读定了，所以不会误存到新打开的笔记上。
  if (mode.value === "edit") {
    void saveDraft();
  }
  void navigate(title);
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

/** 阅读页只显示最新提交；有草稿就提示一下，点按钮才进编辑器看 */
async function refreshDraftHint(title: string) {
  draftExists.value = false;
  try {
    const draft = await invoke<Draft | null>("load_draft", { title });
    draftExists.value = Boolean(draft);
  } catch (error) {
    console.debug("检查草稿失败:", error);
  }
}

/** 只读查看的那一版内容；只有 navigate() 会写它 */
const revisionView = ref<{ rev: number; shortId: string; html: string } | null>(null);

/**
 * 章节是**唯一允许前端自己确定**的部分：正文里的定位属于界面自己的事，
 * 改章节不必再问后端。其余成分一律以后端给的规范地址为准。
 */
const localSection = ref("");

/**
 * 解析失败时保留用户写的那一行。
 *
 * 规范里的例外：`@` 没找到对应版本时（无论是 `NAME@VERSION` 该页没这版，还是
 * `@VERSION` 全局没有），地址栏应当保留用户输入 —— 其余情况一律回显规范全称。
 */
const rejectedAddress = ref("");

/**
 * 地址栏显示的文本。
 *
 * 直接用解析结果里的 `address`（后端已按标准顺序排好），前端不自己拼字符串 ——
 * 于是「语法糖跳转后回显全称」只有一处实现。唯一例外是章节：本地章节叠加上去。
 */
const addressText = computed(() => {
  // 例外：解析失败时显示用户写的原文
  if (rejectedAddress.value) {
    return rejectedAddress.value;
  }

  const address = route.value;
  if (!address || address.kind === "empty") {
    return "";
  }
  if (!localSection.value) {
    return address.address;
  }

  // `#` 与 `$` 都不允许出现在标题里，所以这里按它们切分是安全的
  const withoutSection = address.address.split("#")[0]!;
  const at = address.address.indexOf("$");
  const state = at >= 0 ? address.address.slice(at) : "";
  const base = withoutSection.includes("$")
    ? withoutSection.slice(0, withoutSection.indexOf("$"))
    : withoutSection;
  return `${base}#${localSection.value}${state}`;
});

/**
 * 是否有需要整页覆盖提醒的事。
 *
 * 这些都是「不处理就会误事」的信息（解析失败、读不出来、有未提交的草稿），所以用整页
 * 覆盖而不是角落里的小条。**看历史版本不算**：那是要边看边用的状态，用正文顶部的条。
 */
const noticeOpen = computed(
  () =>
    Boolean(addressError.value) ||
    Boolean(loadError.value) ||
    (!revisionView.value && draftExists.value && !draftHintDismissed.value),
);

/** 关掉覆盖式提示（点空白处或点「知道了」） */
function dismissNotices() {
  addressError.value = "";
  loadError.value = "";
  draftHintDismissed.value = true;
}

/** 回到最新提交 */
function leaveRevision() {
  const title = note.value?.title;
  if (title) {
    void navigate(title);
  }
}

/** 只读查看某一版：地址解析已经确认它存在，这里只取内容 */
async function openRevision(title: string, reference: string) {
  const rev = await invoke<number>("resolve_revision", { title, reference });
  const content = await invoke<RevisionContent>("note_revision", { title, rev });

  // 顺带拿到这一版的缩写（地址栏回显由后端给，这里只是兜底）
  const history = await invoke<{ rev: number; short_id: string }[]>("note_history", {
    title,
  });
  const entry = history.find((item) => item.rev === rev);

  await loadNote(title);
  revisionView.value = { rev, shortId: entry?.short_id ?? reference, html: content.html };
}

/** 草稿预览：草稿也是链上的一版，所以走同一条地址通道 */
async function openDraftPreview() {
  const title = note.value?.title;
  if (!title) {
    return;
  }
  try {
    const draft = await invoke<Draft | null>("load_draft", { title });
    if (draft) {
      await navigate(`${title}@${draft.short_id}`);
    }
  } catch (error) {
    addressError.value = String(error);
  }
}

/** 子页面（标题里有斜杠）的上一级 */
const parentTitle = computed(() => {
  const title = note.value?.title ?? "";
  const cut = title.lastIndexOf("/");
  return cut > 0 ? title.slice(0, cut) : "";
});

/**
 * 记录本地章节。
 *
 * 只有章节允许前端自己确定（正文里定位到哪一节属于界面自己的事），其余成分一律以
 * 后端对地址的解析为准 —— 所以这里只动 localSection，不重新解析地址。
 */
function setSection(id: string) {
  localSection.value = id;
}

/**
 * 地址栏提交 = 导航。语法全在后端，这里不做任何解析。
 */
async function onSubmit(value: string) {
  await navigate(value);
}

/**
 * **唯一的导航入口**：改地址 →（后端）重新解析 → 全量重新加载。
 *
 * 不做局部状态拼接 —— 界面上「在哪」的所有变化（标签栏、内部链接、页头按钮、历史页
 * 导航、草稿预览、创建、退出编辑…）都必须走这里。
 *
 * 解析失败时**不动地址栏**：用户写错了版本引用（`@` 没找到对应版本）时，应当看见自己
 * 输入的内容，而不是被换成他并没有输入的规范地址。
 */
async function navigate(input: string) {
  addressError.value = "";
  rejectedAddress.value = "";

  let address: Address;
  try {
    address = await invoke<Address>("parse_address", { input });
  } catch (error) {
    addressError.value = String(error);
    // 例外：解析不了就保留用户写的那一行，别换成规范地址
    rejectedAddress.value = input.trim();
    console.debug("地址解析失败:", error);
    return;
  }

  // 空地址不是地址：保留当前规范地址，什么都不动
  if (address.kind === "empty") {
    return;
  }

  // 权威副本先落地，其余派生状态都由它决定
  route.value = address;
  revisionView.value = null;
  localSection.value = "";
  confirmDelete.value = false;
  draftHintDismissed.value = false;

  switch (address.kind) {
    case "note":
      await loadNote(address.title);
      return;
    case "edit":
      await loadNote(address.title);
      await beginEditing();
      return;
    case "history":
      await loadNote(address.title);
      return;
    case "revision":
      await openRevision(address.title, address.short_id);
      return;
    case "missing":
      showMissing(address.title);
      return;
    default:
      return;
  }
}

/** 删除：写删除标记 + 把文件挪进 trash/，历史不丢；可勾选顺手回收 */
async function doDelete() {
  const current = note.value;
  if (!current) {
    return;
  }

  busy.value = true;
  try {
    await invoke("delete_note", { title: current.title });
    if (deleteWithGc.value) {
      await invoke("gc", { orphanBlobs: true, supersededDrafts: true });
    }

    confirmDelete.value = false;
    rejectedAddress.value = "";
    note.value = null;
    await refreshNotes();

    const first = notes.value[0];
    if (first) {
      await navigate(first.title);
    } else {
      // 仓库空了：没有地址可去，收掉权威副本
      route.value = null;
      revisionView.value = null;
      loadError.value = "仓库里还没有笔记";
    }
  } catch (error) {
    loadError.value = String(error);
  } finally {
    busy.value = false;
  }
}

/** 回退：把某一版内容作为**新提交**写上去，旧记录一条不改 */
async function onRevert(rev: number) {
  const current = note.value;
  if (!current) {
    return;
  }

  busy.value = true;
  try {
    await invoke<Note>("revert_note", {
      title: current.title,
      rev,
      summary: null,
    });
    draftExists.value = false;
    await refreshNotes();
    // 回退是一个新提交：回到阅读地址，让用户直接看到回退后的内容
    await navigate(current.title);
  } catch (error) {
    console.debug("回退失败:", error);
  } finally {
    busy.value = false;
  }
}

/**
 * 内部链接也走地址解析：是红链还是蓝链，由后端重新判定，前端不自己下结论。
 */
function onWikiLink(payload: { title: string; missing: boolean }) {
  void payload.missing;
  void navigate(payload.title);
}

function onAction(name: string) {
  const title = note.value?.title;
  if (!title) {
    return;
  }

  if (name === "edit") {
    void navigate(`${title}$edit`);
    return;
  }
  if (name === "history") {
    openHistory();
    return;
  }
  if (name === "delete") {
    confirmDelete.value = true;
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
      :title="addressText"
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
            :key="`${note.key}:${note.rev}`"
            :title="note.title"
            :current-rev="note.rev"
            @close="closeHistory"
            @revert="onRevert"
            @open-revision="onOpenRevision"
          />

          <template v-else-if="note">
            <!-- 不是最新提交：明确提示，并给一个回最新的出口 -->
            <!-- 看历史版本是一种状态：做成醒目的条，并在这里给「回退到这一版」 -->
            <div v-if="revisionView" class="revbar">
              <span class="revbar__text">
                正在查看历史版本
                <code>{{ note.title }}@{{ revisionView.shortId }}</code>
                （第 {{ revisionView.rev }} 版，不是最新提交）
              </span>
              <span class="revbar__actions">
                <button type="button" @click="onRevert(revisionView.rev)">
                  回退到这一版
                </button>
                <button type="button" @click="leaveRevision">回到最新版本</button>
              </span>
            </div>

            <PageHeader
              v-if="!revisionView"
              :title="note.title"
              :parent="parentTitle"
              :collapsed="scrolled"
              @action="onAction"
              @open-parent="onNoteSelected"
            />

            <NoteContent
              v-if="!revisionView"
              :html="note.html"
              @wikilink="onWikiLink"
              @section="setSection"
            />

            <NoteContent
              v-if="revisionView"
              :html="revisionView.html"
              @wikilink="onWikiLink"
              @section="setSection"
            />
          </template>

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

  <!-- 提示：整页覆盖，确保被注意到（点空白处可关掉） -->
  <div v-if="noticeOpen" class="app__notices" @click.self="dismissNotices">
    <p v-if="addressError" class="notice notice--error">
      <span>{{ addressError }}</span>
      <button type="button" @click="addressError = ''">知道了</button>
    </p>

    <p v-if="!addressError && loadError" class="notice notice--error">
      <span>{{ loadError }}</span>
    </p>

    <p v-if="!revisionView && draftExists && !draftHintDismissed" class="notice">
      <span>这篇笔记有未提交的草稿（当前显示的是最新提交）</span>
      <button type="button" @click="openDraftPreview">预览</button>
      <button type="button" @click="onAction('edit')">编辑</button>
      <button type="button" @click="draftHintDismissed = true">知道了</button>
    </p>
  </div>

  <!-- 删除确认：整页弹出，必须明确选择才继续 -->
  <div v-if="confirmDelete && note" class="modal" @click.self="confirmDelete = false">
    <div class="modal__card">
      <h2 class="modal__title">删除《{{ note.title }}》？</h2>
      <p class="modal__body">
        历史一条都不会丢：会写一条删除标记，文件挪进 <code>trash/</code>，随时可以捞回来。
      </p>
      <label class="modal__opt">
        <input v-model="deleteWithGc" type="checkbox" />
        顺手回收悬置数据
      </label>
      <div class="modal__actions">
        <button type="button" @click="confirmDelete = false">取消</button>
        <button class="modal__danger" type="button" :disabled="busy" @click="doDelete">
          确认删除
        </button>
      </div>
    </div>
  </div>
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
/* 覆盖式提示：整页遮罩 + 居中卡片，确保被注意到 */
.app__notices {
  position: fixed;
  inset: 0;
  z-index: 55;
  display: flex;
  flex-direction: column;
  gap: 10px;
  align-items: center;
  justify-content: center;
  padding: 24px;
  background: rgb(0 0 0 / 45%);
}

.notice {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
  align-items: center;
  max-width: min(520px, 86vw);
  margin: 0;
  padding: 14px 18px;
  border: 1px solid var(--border);
  border-radius: 10px;
  background: var(--surface);
  color: var(--text);
  font-size: 13px;
  line-height: 1.6;
  box-shadow: 0 20px 60px rgb(0 0 0 / 35%);
}

.notice--error {
  border-color: var(--link-missing);
}

.notice code {
  font-family: var(--mono-font);
  font-size: 12.5px;
  color: var(--text-dim);
}

.notice button {
  appearance: none;
  height: 26px;
  padding: 0 10px;
  border: 1px solid var(--accent-soft);
  border-radius: 6px;
  background: transparent;
  color: var(--accent-soft);
  font-size: 12.5px;
  cursor: pointer;
}

.notice button:hover {
  background: var(--hover);
}

/* 看历史版本：正文顶部的醒目条（不是弹出 —— 要边看边用） */
.revbar {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 12px;
  padding: 10px 14px;
  border: 1px solid var(--accent-soft);
  border-left-width: 3px;
  border-radius: 8px;
  background: var(--surface);
  color: var(--text-dim);
  font-size: 13px;
}

.revbar__text code {
  font-family: var(--mono-font);
  color: var(--text);
}

.revbar__actions {
  display: inline-flex;
  gap: 8px;
}

.revbar__actions button {
  appearance: none;
  height: 26px;
  padding: 0 10px;
  border: 1px solid var(--accent-soft);
  border-radius: 6px;
  background: transparent;
  color: var(--accent-soft);
  font-size: 12.5px;
  cursor: pointer;
}

.revbar__actions button:hover {
  background: var(--hover);
}

/* 删除确认：整页弹出 */
.modal {
  position: fixed;
  inset: 0;
  z-index: 60;
  display: flex;
  align-items: center;
  justify-content: center;
  background: rgb(0 0 0 / 45%);
}

.modal__card {
  width: min(420px, 86vw);
  padding: 20px 22px;
  border: 1px solid var(--border);
  border-radius: 12px;
  background: var(--bg);
  color: var(--text);
  box-shadow: 0 20px 60px rgb(0 0 0 / 35%);
}

.modal__title {
  margin: 0 0 8px;
  font-size: 15px;
}

.modal__body {
  margin: 0 0 14px;
  color: var(--text-dim);
  font-size: 13px;
  line-height: 1.6;
}

.modal__opt {
  display: flex;
  gap: 6px;
  align-items: center;
  margin-bottom: 16px;
  color: var(--text-dim);
  font-size: 13px;
  cursor: pointer;
}

.modal__actions {
  display: flex;
  gap: 8px;
  justify-content: flex-end;
}

.modal__actions button {
  appearance: none;
  height: 30px;
  padding: 0 14px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: transparent;
  color: var(--text);
  font-size: 13px;
  cursor: pointer;
}

.modal__actions button:hover:not(:disabled) {
  background: var(--hover);
}

.modal__actions button:disabled {
  opacity: 0.5;
  cursor: default;
}

.modal__danger {
  border-color: var(--link-missing);
  color: var(--link-missing);
}
</style>
