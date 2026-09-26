<script setup lang="ts">
import { computed, nextTick, onMounted, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import FloatingTools from "./components/FloatingTools.vue";
import HistoryView from "./components/HistoryView.vue";
import MissingNote from "./components/MissingNote.vue";
import NewTab from "./components/NewTab.vue";
import SettingsPage from "./components/SettingsPage.vue";
import AllPages from "./components/AllPages.vue";
import AppMenu from "./components/AppMenu.vue";
import GcPage from "./components/GcPage.vue";
import TrashPage from "./components/TrashPage.vue";
import TaskBar from "./components/TaskBar.vue";
import ViaHint from "./components/ViaHint.vue";
import { labelOf } from "./special";
import { BASE_ZOOM } from "./settings";
import { setThemeMode, themeMode, type ThemeMode } from "./theme";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import NoteContent from "./components/NoteContent.vue";
import NoteEditor from "./components/NoteEditor.vue";
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
  delta_chain_limit: number;
  /** "system" | "light" | "dark" */
  theme: string;
  /** 主题色 #rrggbb */
  accent: string;
  reading_width: number;
  /** 界面缩放（1.0 = 100%），Ctrl + 滚轮调整 */
  zoom: number;
  /** 回收站保留天数（自动清理用） */
  trash_keep_days: number;
  /** 自动回收的间隔天数 */
  gc_interval_days: number;
  /** 上次清理回收站的时间（只读） */
  last_trash_purge: string;
  /** 上次回收的时间（只读） */
  last_gc: string;
}

/** 站点名：顶栏菜单顶上那一行（纯显示，不参与地址） */
const APP_NAME = "重逢笔记";

/** 与 Rust 端 `RevisionContent` 对应（历史里某一版的正文） */
interface RevisionContent {
  rev: number;
  kind: string;
  at: string;
  title: string;
  markdown: string;
  html: string;
}

type Mode =
  | "read"
  | "edit"
  | "missing"
  | "history"
  | "delete"
  | "rollback"
  | "special";

/** 与 Rust 端 `Address` 对应：地址栏那一行的解析结果（后端解析到底） */
type Address =
  | { kind: "empty" }
  | {
      kind: "note";
      title: string;
      address: string;
      /** 指令页面 + `@no-command`：正文按代码块显示（不执行指令） */
      code_block: boolean;
      /** 是跟某条指令来到这一页的（直接打开时为 null） */
      via: { from: string; random: boolean } | null;
    }
  | { kind: "edit"; title: string; address: string }
  | { kind: "history"; title: string; address: string }
  | { kind: "delete"; title: string; address: string }
  | {
      kind: "view-version";
      title: string;
      rev: number;
      id: string;
      short_id: string;
      address: string;
    }
  | {
      kind: "rollback-confirm";
      title: string;
      rev: number;
      id: string;
      short_id: string;
      address: string;
    }
  | {
      kind: "special";
      page: string;
      address: string;
      /** 是跟某条指令来到这一页的（直接打开时为 null） */
      via: { from: string; random: boolean } | null;
    }
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
    case "delete":
      return "delete";
    case "rollback-confirm":
      return "rollback";
    case "special":
      return "special";
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
/** 阅读页只显示最新提交；存在草稿时提示，由用户点开 */
const draftExists = ref(false);
/**
 * 打开着的标签页。每个标签页背后就是一个**地址**，所以整页重解析之后它仍然成立。
 *
 * 以前标签栏列的是仓库里全部笔记（「浏览全部文档」），暂时不做 —— 入口改为
 * `special:newtab`，标签栏只列真正打开着的。
 */
/** 每个标签页自带浏览历史（内部链接是「跳转」，会往里推一条） */
const tabs = ref<
  {
    address: string;
    title: string;
    history: string[];
    cursor: number;
    /** 上次在这个标签页停下的滚动位置（切回来时还原） */
    scroll: number;
  }[]
>([]);
const activeTab = ref(0);
/** 仓库设置（含外观）：唯一来源是后端的 vault.json */
const vaultSettings = ref<VaultSettings | null>(null);

/** 顶栏菜单是否展开 */
const menuOpen = ref(false);

/** 菜单要列的特殊页面：**每次打开都问后端**，所以后端加页面不必重启前端 */
const specialPages = ref<string[]>([]);

/** 当前特殊页的页面名（模板里据此分派；用计算属性避免在模板里碰联合类型） */
const specialPage = computed(() => {
  // 先取到局部变量，联合类型才收窄得动
  const address = route.value;
  return address && address.kind === "special" ? address.page : "";
});
/** 内部链接的右键菜单（坐标来自鼠标事件） */
const linkMenu = ref<{ title: string; x: number; y: number } | null>(null);

/** 标签页上显示什么名字 */
function tabTitleOf(address: Address): string {
  switch (address.kind) {
    case "empty":
      return "";
    case "special":
      return labelOf(address.page);
    default:
      return address.title;
  }
}

/** 把解析结果同步进当前标签页 */
function syncActiveTab(address: Address) {
  if (address.kind === "empty") {
    return;
  }
  const tab = tabs.value[activeTab.value];
  if (tab) {
    tab.address = address.address;
    tab.title = tabTitleOf(address);
    return;
  }
  tabs.value.push({
    address: address.address,
    title: tabTitleOf(address),
    history: [address.address],
    cursor: 0,
    scroll: 0,
  });
}

/** 开一个新标签页。以前这个「+」是「新建笔记」，现在它是真正的开标签页。 */
function openNewTab() {
  tabs.value.push({
    address: "special:newtab",
    title: "新标签页",
    history: ["special:newtab"],
    cursor: 0,
    scroll: 0,
  });
  activeTab.value = tabs.value.length - 1;
  void navigate("special:newtab", "replace");
}

/**
 * 在新标签页打开某个地址（内部链接的 Ctrl/Cmd+点击）。
 *
 * 标题先占位成地址，navigate 之后由解析结果回填 —— 与其它标签页一致。
 */
function openTabWith(address: string) {
  tabs.value.push({
    address,
    title: address,
    history: [address],
    cursor: 0,
    scroll: 0,
  });
  activeTab.value = tabs.value.length - 1;
  void navigate(address, "replace");
}

/** 右键菜单：在新标签页打开 */
function openLinkMenuTarget() {
  const target = linkMenu.value;
  linkMenu.value = null;
  if (target) {
    openTabWith(target.title);
  }
}

/** 右键菜单：复制链接目标（地址栏里能直接粘贴这个写法） */
function copyLinkTarget() {
  const target = linkMenu.value;
  linkMenu.value = null;
  if (target) {
    // 与正文里的复制走同一条路（Tauri 剪贴板插件）
    void writeText(target.title).catch(() => {});
  }
}

/** 切到某个标签页：它带着自己的地址，重新解析一遍（全量重载） */
function selectTab(index: number) {
  const tab = tabs.value[index];
  if (!tab) {
    return;
  }
  activeTab.value = index;
  // 切回标签页：还原它上次停的地方，**不再**执行章节跳转
  void navigate(tab.address, "restore");
}

/** 拖放调整标签页顺序：把 from 位置的标签页挪到 to 位置，并让「当前」仍指向同一个 */
function moveTab(from: number, to: number) {
  if (from === to || from < 0 || to < 0) {
    return;
  }

  const [moved] = tabs.value.splice(from, 1);
  if (!moved) {
    return;
  }
  tabs.value.splice(to, 0, moved);

  if (activeTab.value === from) {
    activeTab.value = to;
  } else if (from < activeTab.value && to >= activeTab.value) {
    activeTab.value -= 1;
  } else if (from > activeTab.value && to <= activeTab.value) {
    activeTab.value += 1;
  }
}

/** 关闭标签页；关掉当前这个就切到邻居，全关了就给一个新的，免得出现没有标签页的空壳 */
/**
 * 抖动信号：每次"关掉最后一个标签、于是又新建了一个"就 +1。
 *
 * 只在**这一条路径**上自增 —— 点加号新建、启动时新建都不该抖（那本来就有明确的动作）。
 */
const shakeTick = ref(0);

function closeTab(index: number) {
  if (tabs.value.length <= 1) {
    tabs.value = [];
    openNewTab();
    // 让新建的这个抖一下：关闭**是**生效了，只是又开了一个。
    // 不抖的话，点了关闭、界面看着没什么变化，用户会以为没反应。
    shakeTick.value += 1;
    return;
  }

  tabs.value.splice(index, 1);
  if (index < activeTab.value) {
    activeTab.value -= 1;
    return;
  }
  if (index === activeTab.value) {
    activeTab.value = Math.min(index, tabs.value.length - 1);
    const next = tabs.value[activeTab.value];
    if (next) {
      void navigate(next.address);
    }
  }
}

/** 草稿提醒被忽略过一次（换地址会重新出现） */
const draftHintDismissed = ref(false);
/** 删除时同时回收孤立数据块与已作废的草稿 */
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
async function loadNote(title: string, codeBlock = false) {
  try {
    // 指令页面的 `@no-command` 走另一条命令：正文包成代码块后再渲染。
    // 单独一条命令而不是给 load_note 加参数，是为了让现有调用点一个都不用动。
    const outcome = await invoke<LoadOutcome>(
      codeBlock ? "load_note_no_command" : "load_note",
      { title },
    );
    if (!outcome.note) {
      showMissing(outcome.title);
      return;
    }

    note.value = outcome.note;
    missingTitle.value = "";
    loadError.value = "";
    scrolled.value = false;
    // `@no-command` 打开的是指令页面本身：问一次后端"这是什么指令"，
    // 顶部据此提示，并给出执行按钮
    void loadCommandInfo(codeBlock ? outcome.note.title : "");
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
    // 建完进编辑器：同样通过改地址（`@edit`）
    await navigate(`${trimmed}@edit`);
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
  // 先留一份原文：指令判断要用它，而它会随提交流程被清空
  const markdown = draftText.value;
  try {
    const committed = await invoke<Note>("commit_note", {
      title: current.title,
      markdown,
      summary: summary || null,
      baseRev: current.rev,
    });

    editorStatus.value = "";
    await refreshNotes();
    scrolled.value = false;
    scrollEl.value?.scrollTo({ top: 0 });

    // 提交之后回到「阅读最新提交」这个地址 —— 状态变化同样只走地址。
    // 指令页面要落在 `@no-command` 上：否则刚提交完就被自己的重定向带走，
    // 连"提交到底成没成"都看不清。判断交给后端（规则只有一处）。
    const command = await invoke<string | null>("command_kind", { markdown });
    await navigate(command ? `${committed.title}@no-command` : committed.title);
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
    editorStatus.value = "草稿已丢弃，已恢复为上一次提交的内容";
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

/** 看版本历史：同样是改地址（`@history`） */
function openHistory() {
  const title = note.value?.title;
  if (title) {
    void navigate(`${title}@history`);
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
    await navigate(`${renamed.title}@edit`);
    editorStatus.value = `已改名为「${renamed.title}」；本次改名记为一版`;
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
    vaultSettings.value = settings;
    applyAppearance();
    // 到点了就清回收站 / 回收内容块；判定由后端按"上次执行时间"做
    void runMaintenanceOnce();
    vaultRoot.value = settings.root;
  } catch (error) {
    console.debug("读取仓库设置失败:", error);
  }

  try {
    await refreshNotes();
    // 入口是 `special:newtab`：暂时不列全部文档，所以启动也不再自动打开第一篇
    openNewTab();
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

  // 记下当前标签页的浏览位置，切走再回来时还原
  const tab = tabs.value[activeTab.value];
  if (tab) {
    tab.scroll = scrollTop;
  }

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
    void navigate(`${title}@view-${reference}`);
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

/** 后退 / 前进：只在当前标签页的历史里移动，不产生新记录 */
function goBack() {
  const tab = tabs.value[activeTab.value];
  if (!tab || tab.cursor <= 0) {
    return;
  }
  tab.cursor -= 1;
  const target = tab.history[tab.cursor];
  if (target) {
    void navigate(target, "history");
  }
}

function goForward() {
  const tab = tabs.value[activeTab.value];
  if (!tab || tab.cursor >= tab.history.length - 1) {
    return;
  }
  tab.cursor += 1;
  const target = tab.history[tab.cursor];
  if (target) {
    void navigate(target, "history");
  }
}

const canGoBack = computed(() => {
  const tab = tabs.value[activeTab.value];
  return Boolean(tab && tab.cursor > 0);
});

const canGoForward = computed(() => {
  const tab = tabs.value[activeTab.value];
  return Boolean(tab && tab.cursor < tab.history.length - 1);
});

async function onMenu() {
  menuOpen.value = !menuOpen.value;
  if (!menuOpen.value) {
    return;
  }
  // 展开时才问一次：菜单里的条目永远与后端一致
  try {
    specialPages.value = await invoke<string[]>("special_pages");
  } catch {
    specialPages.value = [];
  }
}

/**
 * 自动维护只提交一次（应用启动后）。
 *
 * 间隔判定在后端（用上次执行时间比），这里只负责"启动时提交一次"；**跑在后台**，
 * 做了什么会出现在底部任务栏 —— 自动删数据这件事必须能被看到，而不是只写进控制台。
 */
let maintenanceRequested = false;

async function runMaintenanceOnce() {
  if (maintenanceRequested) {
    return;
  }
  maintenanceRequested = true;
  try {
    // 没到点后端会返回 null，并且**不建任务** —— 任务栏因此不会每次启动都弹一条空任务
    const task = await invoke<number | null>("submit_maintenance");
    if (task === null) {
      console.debug("数据库维护：还没到时间，跳过");
    }
  } catch (error) {
    // 提交失败不该拦住用户用应用
    console.debug("自动维护提交失败:", error);
  }
}

/**
 * 重新读取设置。
 *
 * 后台任务（维护 / 回收）会改写"上次执行时间"，界面上的值必须跟着更新 ——
 * 否则设置页会一直显示任务执行之前的旧时间。
 */
/**
 * 当前 `@no-command` 页面的指令信息；不是指令页面时为 null。
 *
 * 短名、中文名、说明全部由后端那张指令表给出 —— 前端不解释 `$$COMMAND$$`，
 * 也不维护第二份命令清单。
 */
const commandInfo = ref<{
  kind: string;
  label: string;
  detail: string;
} | null>(null);

async function loadCommandInfo(title: string) {
  if (!title) {
    commandInfo.value = null;
    return;
  }
  try {
    commandInfo.value = await invoke<typeof commandInfo.value>("command_info", { title });
  } catch (error) {
    console.debug("读取指令信息失败:", error);
    commandInfo.value = null;
  }
}

/**
 * 执行这条指令：回到不带 `@no-command` 的地址。
 *
 * 执行仍在后端（地址一解析就按指令跳），前端只负责"点一下把它送回去解析" ——
 * 命令与页面查找因此是解耦的：同一张表既回答"这是什么"，也决定"跳到哪"。
 */
function runCommand() {
  const title = currentTitle.value;
  if (title) {
    void navigate(title);
  }
}

/**
 * 标题下方的来源提示：跟重定向来到这一页时才显示。
 *
 * 随机跳转不写来源页名 —— 那个名字不是"目标"，写了反而误导；只说"来自随机重定向"。
 */
const viaHint = computed(() => {
  const address = route.value;
  // 笔记页与特殊页面都要标（虚拟命名空间下的页面同样是"页面"）
  const via =
    address && (address.kind === "note" || address.kind === "special")
      ? address.via
      : null;
  if (!via) {
    return "";
  }
  return via.random ? "来自随机重定向" : `重定向自《${via.from}》`;
});

async function reloadSettings() {
  try {
    vaultSettings.value = await invoke<VaultSettings>("get_settings");
    applyAppearance();
  } catch (error) {
    console.debug("重新读取设置失败:", error);
  }
}

/** 菜单里点一项：跳过去（进历史）并收起菜单 */
function openFromMenu(address: string) {
  menuOpen.value = false;
  void navigate(address);
}

/**
 * 把外观设置套到文档上。
 *
 * - 主题：`system` 由 JS 解析成实际值（CSS 里刻意没有 prefers-color-scheme 媒体查询，
 *   深色是默认态），所以这里要监听系统变化；
 * - 主题色：覆盖 `--accent` / `--accent-soft` 两个 token，其余（选中色等）保持主题默认；
 * - 限宽：覆盖 `--reading-width`。
 */
function applyAppearance() {
  const appearance = vaultSettings.value;
  if (!appearance) {
    return;
  }

  // 主题**不在这里落地**：`theme.ts` 是它唯一的真相（标题栏按钮、index.html 的
  // 防闪烁脚本、跟随系统的实时响应都在那儿）。这里只把后端的值交给它。
  setThemeMode(appearance.theme as ThemeMode);

  const root = document.documentElement.style;
  root.setProperty("--accent", appearance.accent);
  root.setProperty("--accent-soft", appearance.accent);
  root.setProperty("--accent-tint", tintOf(appearance.accent));
  root.setProperty("--reading-width", `${appearance.reading_width}px`);

  // 界面缩放交给 WebView 自己做：整页等比，和浏览器一致。
  // 生效的是"基准 × 用户缩放"：基准负责把默认字号整体抬高，用户值只做相对调整。
  void getCurrentWebview().setZoom(appearance.zoom * BASE_ZOOM);
}

/** 设置页改了哪一项就只传哪一项（后端是补丁式更新） */
async function updateSettings(patch: Record<string, unknown>) {
  // 主题是一条共享真相：先在本地落地（标题栏那个轮换按钮立刻跟上），再写回后端
  if (typeof patch.theme === "string") {
    setThemeMode(patch.theme as ThemeMode);
  }

  try {
    vaultSettings.value = await invoke<VaultSettings>("update_settings", patch);
    applyAppearance();
  } catch (error) {
    addressError.value = String(error);
  }
}

/** 标题栏的轮换按钮：改的是同一条真相，落盘统一交给下面的 watcher */
function onTitlebarTheme(mode: ThemeMode) {
  void updateSettings({ theme: mode });
}

// 任何来源改了主题（标题栏按钮 / 设置页）都落回 preferences.json。
// 判等在这里，所以两边不会来回打架。
watch(themeMode, (mode) => {
  if (vaultSettings.value && vaultSettings.value.theme !== mode) {
    void updateSettings({ theme: mode });
  }
});

/**
 * 当前地址指向的笔记标题；特殊页面、还不存在的页面、空地址都是 null。
 *
 * 用计算属性集中判断，免得每个用到它的地方各写一遍 switch。
 */
const currentTitle = computed(() => {
  const address = route.value;
  if (!address) {
    return null;
  }
  switch (address.kind) {
    case "note":
    case "edit":
    case "history":
    case "delete":
    case "view-version":
    case "rollback-confirm":
      return address.title;
    default:
      return null;
  }
});

/**
 * 是否显示「编辑」按钮：有这篇笔记、且不在编辑中。
 *
 * 「编辑」永远编辑**最新提交**（编辑器只有一份当前内容），所以看历史版本时它也指同一件事。
 * 还不存在的页面不给这个按钮 —— `@edit` 对它没有意义（那条路要先建它）。
 */
const canEdit = computed(() => Boolean(currentTitle.value) && mode.value !== "edit");

/** 直接进入编辑：仍然是"改地址"，不是直接切状态 */
function beginEditingNow() {
  const title = currentTitle.value;
  if (title) {
    void navigate(`${title}@edit`);
  }
}

/** `special:all` 的页号（地址里的 `#`） */
const allPagesSection = computed(() => {
  const address = route.value;
  return address && address.kind === "special" && address.page === "all"
    ? sectionOf(address)
    : "";
});

/**
 * 翻到某一页：**第 1 页回到不带 `#` 的形式**（能省则省），其余写进地址。
 * 默认是按「跳转」入历史，所以翻页可以后退回去。
 */
function goToAllPage(page: number) {
  void navigate(page > 1 ? `special:all#${page}` : "special:all");
}

/** 地址正指向设置页里的哪一项（`special:settings#accent`） */
const settingsFocus = computed(() => {
  const address = route.value;
  return address ? sectionOf(address) : "";
});

/** 列表里点一条（special:all 等）：算「跳转」，进历史 —— 与正文内部链接一致 */
function openFromList(address: string) {
  void navigate(address);
}

/**
 * `#rrggbb` → 低透明度版本。
 *
 * 主题色在设置里是**不透明** hex，而"表头底色"这类淡染需要透明版本；CSS 里没法对
 * 运行时设的变量做混色（且不能假定支持 color-mix），所以在 JS 里算好一个变量。
 */
function tintOf(hex: string): string {
  const match = /^#([0-9a-fA-F]{6})$/.exec(hex.trim());
  if (!match) {
    return hex;
  }
  const value = Number.parseInt(match[1]!, 16);
  return `rgba(${(value >> 16) & 255}, ${(value >> 8) & 255}, ${value & 255}, 0.16)`;
}

/**
 * Ctrl + 滚轮缩放界面。
 *
 * 用 WebView 自己的缩放（整页等比），而不是逐处改 `font-size` —— 后者要动每一处字号，
 * 而且图片、间距不会跟着变。值存进 `preferences.json`（界面偏好），重启后保持。
 *
 * 立即生效保手感，落盘节流：滚轮一次会连发很多事件，逐个写文件既慢也没意义。
 */
const ZOOM_MIN = 0.5;
const ZOOM_MAX = 3;
const ZOOM_STEP = 0.1;
let zoomSaveTimer: number | undefined;

function onWheelZoom(event: WheelEvent) {
  if (!event.ctrlKey) {
    return;
  }
  // 拦掉 WebView 自己的 Ctrl+滚轮行为，避免两套缩放打架
  event.preventDefault();

  const current = vaultSettings.value?.zoom ?? 1;
  const next = Math.min(
    ZOOM_MAX,
    Math.max(ZOOM_MIN, current - Math.sign(event.deltaY) * ZOOM_STEP),
  );
  if (next === current) {
    return;
  }

  if (vaultSettings.value) {
    vaultSettings.value.zoom = next;
  }
  void getCurrentWebview().setZoom(next);

  window.clearTimeout(zoomSaveTimer);
  zoomSaveTimer = window.setTimeout(() => void updateSettings({ zoom: next }), 400);
}

window.addEventListener("wheel", onWheelZoom, { passive: false });

/** 标签栏底部的设置入口 */
function openSettings() {
  void navigate("special:settings");
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

/** 只读查看的那一版内容（地址 `名称@view-缩写` 时才有）；只有 navigate() 会写它 */
const revisionView = ref<{ rev: number; shortId: string; html: string } | null>(null);

/**
 * 回退确认页要回退到哪一版（地址 `名称@rollback-缩写` 时才有）。
 *
 * 同样只有 navigate() 写它 —— 确认页本身也是一个地址，所以「要回退到哪一版」在地址里。
 */
const rollbackTarget = ref<{ rev: number; shortId: string } | null>(null);

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

/**
 * 地址栏显示的文本。
 *
 * 直接用解析结果里的 `address`（后端已按标准顺序排好），前端不自己拼字符串 ——
 * 于是「语法糖跳转后回显全称」只有一处实现。唯一例外是章节：本地章节叠加上去。
 */
const addressText = computed(() => {
  // 例外：解析失败时显示用户写的原文

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
 * 两类提示，**刻意不共用同一个框**：
 *
 * - **错误**（解析失败、读不出来）：整页覆盖 —— 不处理就会误事，必须被看到；
 * - **状态提示**（有未提交的草稿）：顶部悬挂条 —— 只是提醒，不该挡住正文。
 *
 * 「正在查看历史版本」不属于这两类：它是正文顶部的常驻条，要边看边用。
 */
const errorNotice = computed(() => addressError.value || loadError.value);

const hintNotice = computed(
  () => !revisionView.value && draftExists.value && !draftHintDismissed.value,
);

/** 关掉覆盖式错误提示（点空白处或点「知道了」） */
function dismissNotices() {
  addressError.value = "";
  loadError.value = "";
}

/** 从规范地址里取章节（`NAME[@STATE][#章节]` 的最后一段） */
function sectionOf(address: Address): string {
  if (address.kind === "empty") {
    return "";
  }
  const hash = address.address.indexOf("#");
  return hash >= 0 ? address.address.slice(hash + 1) : "";
}

/**
 * 落到正文之后的滚动位置。
 *
 * - **切回标签页**（`restore`）：回到它上次停的地方 —— 所以 `示例笔记#代码`
 *   每个标签页只跳一次，切走再回来保持原来的浏览位置；
 * - 其余情况：地址里带 `#章节` 就跳过去，否则回到顶部。
 */
async function settleScroll(movement: string, address: Address) {
  await nextTick();
  const tab = tabs.value[activeTab.value];

  if (movement === "restore") {
    scrollEl.value?.scrollTo({ top: tab?.scroll ?? 0 });
    return;
  }

  const section = sectionOf(address);
  if (section) {
    let id = section;
    try {
      id = decodeURIComponent(section);
    } catch {
      // 非法转义序列就按原样找
    }
    document.getElementById(id)?.scrollIntoView({ block: "start" });
    return;
  }
  scrollEl.value?.scrollTo({ top: 0 });
}

/** 回到最新提交 */
function leaveRevision() {
  const title = note.value?.title;
  if (title) {
    void navigate(title);
  }
}

/** 从确认页取消：回退确认取消时回到「查看那一版」（用户本来就是从那里点过来的） */
function cancelConfirm() {
  const title = note.value?.title;
  if (!title) {
    return;
  }
  if (mode.value === "rollback" && rollbackTarget.value) {
    void navigate(`${title}@view-${rollbackTarget.value.shortId}`);
    return;
  }
  void navigate(title);
}

/** 只读查看某一版：地址解析已经确认它存在，这里只取内容 */


/**
 * 只读查看某一版。
 *
 * 地址（`名称@view-缩写`）里已经有版本号与缩写，所以这里只取内容。
 */
async function openRevision(title: string, rev: number, shortId: string) {
  const content = await invoke<RevisionContent>("note_revision", { title, rev });
  await loadNote(title);
  revisionView.value = { rev, shortId, html: content.html };
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
      await navigate(`${title}@view-${draft.short_id}`);
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
  // 地址栏输入是「替换当前这条」，不产生新的历史记录
  await navigate(value, "replace");
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
async function navigate(
  input: string,
  movement: "push" | "replace" | "history" | "restore" = "push",
) {
  addressError.value = "";

  let address: Address;
  try {
    address = await invoke<Address>("parse_address", { input });
  } catch (error) {
    addressError.value = String(error);
    // 统一规则：**用户输入的地址一旦报错，就退回修改前的地址**，不把用户输入留在
    // 地址栏（以前为「@ 版本没找到」留的那条例外取消了）。
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
  rollbackTarget.value = null;
  localSection.value = "";
  draftHintDismissed.value = false;
  linkMenu.value = null;

  // 草稿提示是**上一条笔记**的状态，每次地址解析后先归零：
  // 否则切到 special:newtab 之类的地方，上一条笔记的"有未提交草稿"会跟着飘过来。
  // 需要它的分支（阅读）随后会用 refreshDraftHint 重新判定。
  draftExists.value = false;
  syncActiveTab(address);

  // 记进当前标签页的浏览历史：内部链接＝跳转（推一条，并丢掉原来的前进部分）；
  // 地址栏输入＝替换当前这条；前进后退＝只移动游标。
  const tab = tabs.value[activeTab.value];
  if (tab && movement === "push") {
    tab.history = tab.history.slice(0, tab.cursor + 1);
    tab.history.push(address.address);
    tab.cursor = tab.history.length - 1;
  } else if (tab && movement === "replace") {
    tab.history[tab.cursor] = address.address;
  }

  switch (address.kind) {
    case "special":
      // 特殊页面：后端只负责解析出来，内容由前端渲染
      return;
    case "note":
      await loadNote(address.title, address.code_block);
      await settleScroll(movement, address);
      return;
    case "edit":
      await loadNote(address.title);
      await beginEditing();
      return;
    case "history":
      await loadNote(address.title);
      return;
    case "delete":
      // 删除的二次确认页：确认状态本身也在地址里
      await loadNote(address.title);
      return;
    case "view-version":
      await openRevision(address.title, address.rev, address.short_id);
      await settleScroll(movement, address);
      return;
    case "rollback-confirm":
      await loadNote(address.title);
      rollbackTarget.value = { rev: address.rev, shortId: address.short_id };
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


/**
 * 要求回退到某一版：**先去确认页**（`名称@rollback-缩写`），确认后才真的动手。
 *
 * 回退虽然不丢历史，但会写一个新提交，所以不直接执行 —— 与删除一样先落到一页确认上，
 * 而这一页本身也是地址。
 */
function onRollbackRequire(reference: string) {
  const title = note.value?.title;
  if (title && reference) {
    void navigate(`${title}@rollback-${reference}`);
  }
}

/** 确认回退：把那一版内容作为**新提交**写上去，旧记录一条不改 */
async function doRollback() {
  const current = note.value;
  const target = rollbackTarget.value;
  if (!current || !target) {
    return;
  }

  busy.value = true;
  try {
    await invoke<Note>("revert_note", {
      title: current.title,
      rev: target.rev,
      summary: null,
    });
    draftExists.value = false;
    await refreshNotes();
    // 回到阅读地址，让用户直接看到回退后的内容
    await navigate(current.title);
  } catch (error) {
    addressError.value = String(error);
  } finally {
    busy.value = false;
  }
}

/**
 * 内部链接也走地址解析：是红链还是蓝链，由后端重新判定，前端不自己下结论。
 */
function onWikiLink(payload: { title: string; missing: boolean }) {
  void payload.missing;
  // 内部链接算「跳转」，所以可以后退回上一条
  void navigate(payload.title, "push");
}

function onAction(name: string) {
  const title = note.value?.title;
  if (!title) {
    return;
  }

  if (name === "edit") {
    void navigate(`${title}@edit`);
    return;
  }
  if (name === "history") {
    openHistory();
    return;
  }
  if (name === "delete") {
    // 删除不是弹出框，是一页地址：`名称@delete`
    void navigate(`${title}@delete`);
    return;
  }
  console.debug("page action:", name);
}
</script>

<template>
  <WindowResizeHandles />

  <div class="app">
    <TitleBar
      :title="addressText"
      :can-back="canGoBack"
      :can-forward="canGoForward"
      @back="goBack"
      @forward="goForward"
      @menu="onMenu"
      @theme="onTitlebarTheme"
      @submit="onSubmit"
    />

    <div class="app__main">
      <TabRail
        :tabs="tabs"
        :active="activeTab"
        :shake-tick="shakeTick"
        @select="selectTab"
        @close="closeTab"
        @new-tab="openNewTab"
        @settings="openSettings"
        @move="moveTab"
      />

      <main
        ref="scrollEl"
        class="app__body"
        :class="{
          'app__body--wide': !limitWidth,
          'app__body--fit': mode === 'edit',
        }"
        @scroll.passive="onScroll"
      >
      <!-- 特殊命名空间下的页面也要标出来源：与笔记页共用 ViaHint，文案与样式只写一次 -->
      <ViaHint v-if="mode === 'special'" :hint="viaHint" />

        <div class="app__column" :class="{ 'app__column--wide': !limitWidth }">
          <!-- 特殊页面：由前端渲染（后端只负责把地址解析成 Special） -->
          <NewTab
        v-if="mode === 'special' && specialPage === 'newtab'"
        @open="onSubmit"
      />
      <AllPages
        v-else-if="mode === 'special' && specialPage === 'all'"
        :page-section="allPagesSection"
        @open="openFromList"
        @open-new="openTabWith"
        @page="goToAllPage"
      />
      <SettingsPage
        v-else-if="mode === 'special' && specialPage === 'settings' && vaultSettings"
        :settings="vaultSettings"
        :focus="settingsFocus"
        @update="updateSettings"
      />
      <GcPage v-else-if="mode === 'special' && specialPage === 'gc'" />
      <TrashPage
        v-else-if="mode === 'special' && specialPage === 'trash'"
        :keep-days="vaultSettings?.trash_keep_days ?? 30"
        :last-purge="vaultSettings?.last_trash_purge ?? ''"
        @open="openFromList"
      />

          <!-- 编辑中：不显示页头，操作都在编辑器自己那一行里 -->
          <NoteEditor
            v-else-if="mode === 'edit' && note"
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
            @rollback="onRollbackRequire"
            @open-revision="onOpenRevision"
          />

          <!-- 删除的二次确认：一页地址（`名称@delete`），不再是弹出框 -->
          <template v-else-if="mode === 'delete' && note">
            <section class="confirm">
              <h2 class="confirm__title">删除《{{ note.title }}》？</h2>
              <p class="confirm__body">
                将写入一条删除标记，并把笔记文件移入 <code>trash/</code>，
                历史版本不会丢失。当前不提供恢复入口；如需取回，可在文件系统中手动移回 notes/ 目录。
              </p>
              <label class="confirm__opt">
                <input v-model="deleteWithGc" type="checkbox" />
                同时回收孤立数据块与已作废的草稿
              </label>
              <div class="confirm__actions">
                <button type="button" @click="cancelConfirm">取消</button>
                <button
                  class="confirm__danger"
                  type="button"
                  :disabled="busy"
                  @click="doDelete"
                >
                  确认删除
                </button>
              </div>
            </section>
          </template>

          <!-- 回退的二次确认：`名称@rollback-缩写` -->
          <template v-else-if="mode === 'rollback' && note && rollbackTarget">
            <section class="confirm">
              <h2 class="confirm__title">
                回退到版本 {{ rollbackTarget.rev }}（{{ rollbackTarget.shortId }}）？
              </h2>
              <p class="confirm__body">
                内容取自 <code>{{ note.title }}@view-{{ rollbackTarget.shortId }}</code>
                ，作为<strong>一次新的提交</strong>写入；原有版本不会被修改，历史中会新增一版。
              </p>
              <div class="confirm__actions">
                <button type="button" @click="cancelConfirm">取消</button>
                <button
                  class="confirm__danger"
                  type="button"
                  :disabled="busy"
                  @click="doRollback"
                >
                  确认回退
                </button>
              </div>
            </section>
          </template>

          <template v-else-if="note">
            <!-- 不是最新提交：明确提示，并给一个回最新的出口 -->
            <!-- 看历史版本是一种状态：做成醒目的条，并在这里给「回退到这一版」 -->
            <div v-if="revisionView" class="revbar">
              <span class="revbar__text">
                正在查看历史版本
                <code>{{ note.title }}@{{ revisionView.shortId }}</code>
                （第 {{ revisionView.rev }} 版，非最新提交）
              </span>
              <span class="revbar__actions">
                <button
                  type="button"
                  @click="onRollbackRequire(revisionView.shortId)"
                >
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
              :under="viaHint"
              @action="onAction"
              @open-parent="onNoteSelected"
            />

            <!--
              指令页面用 `@no-command` 打开时：说清这是什么指令，并给一个"执行"的出口。
              认不出的指令不给执行按钮 —— 它执行不了，该做的是点右上角去修。
            -->
            <div
              v-if="!revisionView && commandInfo"
              class="cmdbar"
              :class="{ 'cmdbar--bad': commandInfo.kind === 'unrecognized' }"
            >
              <span class="cmdbar__label">{{ commandInfo.label }}</span>
              <span class="cmdbar__detail">{{ commandInfo.detail }}</span>
              <button
                v-if="commandInfo.kind !== 'unrecognized'"
                class="cmdbar__run"
                type="button"
                @click="runCommand"
              >
                执行
              </button>
            </div>

            <NoteContent
              v-if="!revisionView"
              :html="note.html"
              @wikilink="onWikiLink"
              @wikilink-new="openTabWith"
              @wikilink-menu="linkMenu = $event"
              @section="setSection"
            />

            <NoteContent
              v-if="revisionView"
              :html="revisionView.html"
              @wikilink="onWikiLink"
              @wikilink-new="openTabWith"
              @wikilink-menu="linkMenu = $event"
              @section="setSection"
            />
          </template>

          <p v-else class="app__empty">仓库位置：{{ vaultRoot || "（未能读取）" }}</p>
        </div>
      </main>
    </div>
  </div>

  <AppMenu
    :open="menuOpen"
    :pages="specialPages"
    :title="APP_NAME"
    @open="openFromMenu"
    @close="menuOpen = false"
  />

  <TaskBar @finished="reloadSettings" />

  <FloatingTools
    :limited="limitWidth"
    :can-edit="canEdit"
    @edit="beginEditingNow"
    @toggle-width="toggleWidth"
    @scroll-top="scrollToTop"
    @scroll-bottom="scrollToBottom"
  />

  <!-- 内部链接的右键菜单：点空白处即关掉 -->
  <div v-if="linkMenu" class="linkmenu-backdrop" @click.self="linkMenu = null">
    <div class="linkmenu" :style="{ left: `${linkMenu.x}px`, top: `${linkMenu.y}px` }">
      <p class="linkmenu__target">{{ linkMenu.title }}</p>
      <button type="button" @click="openLinkMenuTarget">在新标签页打开</button>
      <button type="button" @click="copyLinkTarget">复制链接目标</button>
    </div>
  </div>

  <!-- 错误：整页覆盖（这一类必须被看到，点空白处关掉） -->
  <div v-if="errorNotice" class="error-cover" @click.self="dismissNotices">
    <p class="notice notice--error">
      <span>{{ errorNotice }}</span>
      <button type="button" @click="dismissNotices">关闭</button>
    </p>
  </div>

  <!-- 状态提示：顶部悬挂条（只是提醒，不挡正文） -->
  <div v-if="hintNotice" class="hint-bar">
    <span>此笔记存在未提交的草稿；当前显示的是最新提交。</span>
    <span class="hint-bar__actions">
      <button type="button" @click="openDraftPreview">预览</button>
      <button type="button" @click="onAction('edit')">编辑</button>
      <button type="button" @click="draftHintDismissed = true">关闭</button>
    </span>
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
/* 一、错误：整页遮罩 + 居中卡片（不处理就会误事） */
.error-cover {
  position: fixed;
  inset: 0;
  z-index: 55;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 24px;
  background: rgb(0 0 0 / 45%);
}

/* 二、状态提示：顶部悬挂条（不挡正文，也不抢焦点的视觉重心） */
.hint-bar {
  position: fixed;
  top: calc(var(--titlebar-height) + 10px);
  left: 50%;
  z-index: 46;
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
  align-items: center;
  max-width: min(720px, 88vw);
  padding: 9px 14px;
  border: 1px solid var(--accent-soft);
  border-left-width: 3px;
  border-radius: 8px;
  background: var(--surface);
  color: var(--text-dim);
  font-size: 13px;
  box-shadow: 0 10px 30px rgb(0 0 0 / 25%);
  transform: translateX(-50%);
}

.hint-bar__actions {
  display: inline-flex;
  gap: 8px;
}

.hint-bar__actions button {
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

.hint-bar__actions button:hover {
  background: var(--hover);
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
/* 确认页（删除 / 回退）：也是一页地址，不再用弹出框 */
.confirm {
  max-width: 560px;
  margin: 24px auto 0;
  padding: 20px 22px;
  border: 1px solid var(--border);
  border-left-width: 3px;
  border-radius: 10px;
  background: var(--surface);
}

.confirm__title {
  margin: 0 0 10px;
  color: var(--text);
  font-size: 16px;
}

.confirm__body {
  margin: 0 0 16px;
  color: var(--text-dim);
  font-size: 13.5px;
  line-height: 1.7;
}

.confirm__body code {
  font-family: var(--mono-font);
  color: var(--text);
}

.confirm__opt {
  display: flex;
  gap: 6px;
  align-items: center;
  margin-bottom: 18px;
  color: var(--text-dim);
  font-size: 13px;
  cursor: pointer;
}

.confirm__actions {
  display: flex;
  gap: 8px;
  justify-content: flex-end;
}

.confirm__actions button {
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

.confirm__actions button:hover:not(:disabled) {
  background: var(--hover);
}

.confirm__actions button:disabled {
  opacity: 0.5;
  cursor: default;
}

.confirm__danger {
  border-color: var(--link-missing);
  color: var(--link-missing);
}
/* 内部链接的右键菜单 */
.linkmenu-backdrop {
  position: fixed;
  inset: 0;
  z-index: 58;
}

.linkmenu {
  position: fixed;
  display: flex;
  flex-direction: column;
  min-width: 168px;
  padding: 6px;
  border: 1px solid var(--border);
  border-radius: 8px;
  background: var(--surface);
  box-shadow: 0 12px 32px rgb(0 0 0 / 28%);
}

/* ---------- 指令页面提示条（`@no-command`） ---------- */

.cmdbar {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 10px;
  margin: 10px 0 14px;
  padding: 8px 12px;
  border: 1px solid var(--border);
  border-radius: 8px;
  background: var(--surface);
  font-size: 13px;
}

.cmdbar__label {
  flex-shrink: 0;
  padding: 0 7px;
  border-radius: 9px;
  background: var(--code-bg);
  color: var(--text-dim);
  font-size: 12px;
}

.cmdbar__detail {
  color: var(--text-dim);
  overflow-wrap: anywhere;
}

.cmdbar__run {
  margin-left: auto;
  flex-shrink: 0;
  padding: 3px 12px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background-color: transparent;
  color: var(--text);
  font-size: 12.5px;
  cursor: pointer;
}

.cmdbar__run:hover {
  background-color: var(--hover);
}

/* 认不出的指令：一执行就报错，用缺失链接的颜色提醒 */
.cmdbar--bad .cmdbar__label {
  color: var(--link-missing);
}

.linkmenu__target {
  margin: 0 0 4px;
  padding: 4px 8px;
  color: var(--text-dim);
  font-size: 12px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.linkmenu button {
  appearance: none;
  padding: 6px 8px;
  border: 0;
  border-radius: 5px;
  background: transparent;
  color: var(--text);
  font-size: 13px;
  text-align: left;
  cursor: pointer;
}

.linkmenu button:hover {
  background: var(--hover);
}
</style>

<!--
  全局规则（非 scoped）：编辑页的高度必须**沿链逐级传**，否则两栏的高度由内容撑开，
  页面就成了唯一的滚动容器（点预览里的锚点会让两栏一起动，就是这个现象）。

  这里用**模板显式加的类** `.app__body--fit`，而不是 `:has(.editor)`：
  后者在运行环境的 WebKitGTK 里若不支持，整条规则会被丢弃，于是"看起来写了却没生效"。
  显式类没有这个不确定性。
-->
<style>
.app__body--fit {
  display: flex;
  flex-direction: column;
  overflow: hidden;
}

/* 只作用直接子层：它可能是阅读栏，也可能直接就是编辑器 —— 不必知道类名 */
.app__body--fit > * {
  display: flex;
  flex: 1 1 auto;
  flex-direction: column;
  min-height: 0;
}

/* 编辑器吃掉剩余高度。用 flex 而不是 height:100% —— 不依赖父级是"确定高度"的百分比 */
.app__body--fit .editor {
  flex: 1 1 auto;
  min-height: 0;
}
</style>

