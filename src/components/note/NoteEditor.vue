<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { Check, ImagePlus, RotateCcw, Save, Trash2, X } from "@lucide/vue";
import { open } from "@tauri-apps/plugin-dialog";
import {
    policyFrom,
    policyLabel,
    type Draft,
    type Note,
    type Policy,
    type Reading,
} from "../../bindings/note.ts";
import type { Uploaded } from "../../bindings/files.ts";
import { fileReferenceOf } from "../../view/file-links.ts";
import { clipboardFiles, uploadPasted } from "../../view/paste-files.ts";
import { gpgAvailable, protection } from "../../core/preferences.ts";
import { applyLineNumbers, codeLineNumbers, highlightCode } from "../../view/code-blocks.ts";
import { decorateNoteHtml } from "../../view/note-html.ts";
// `codemirror` 是元包（提供 basicSetup 等），EditorState 由 @codemirror/state 提供 ——
// 后者必须作为**直接依赖**安装：pnpm 的严格 node_modules 下，传递依赖不可直接导入。
import { basicSetup } from "codemirror";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { sourceExtensions } from "../../view/editor-setup.ts";

/**
 * 笔记编辑器。
 *
 * 左栏是 CodeMirror 6 的源码视图，右栏是后端渲染出来的预览；上面一排真按钮：
 * 保存草稿、提交、放弃草稿、取消。源码由这个组件自己持有 —— 装载笔记、恢复草稿、
 * 自动保存都基于它。
 *
 * 「提交完成」与「取消」都要换地址，地址归上层管，所以这里只把**要去的地址**派发出去。
 */
const props = defineProps<{
    /** 正在编辑的笔记标题 */
    title: string;
}>();

const emit = defineEmits<{
    /** 离开编辑器（提交完成或取消）后要去的地址：这篇笔记的阅读地址 */
    (e: "navigate", title: string): void;
}>();

/** 编辑器里的源码。由这里持有：装载、恢复草稿、自动保存都基于它 */
const markdown = ref("");
/** 上一次提交的内容：草稿与它一致就没什么可存的 */
const committedMarkdown = ref("");
/** 笔记装载成功了没有 —— 没成功就不做保存 / 提交这类写操作 */
const hasNote = ref(false);
/** 槽位里有没有草稿 —— 草稿是每篇一个可覆盖槽位，只有"有 / 没有" */
const hasDraft = ref(false);
/** 正在装载或提交时禁掉按钮，避免连点 */
const busy = ref(false);
/** 装载中 */
const loading = ref(false);
/** 装载失败的原因 */
const loadProblem = ref("");

/** 这一篇当前版读不出来：需要口令 */
const locked = ref(false);

/** 上次输的那把是错的（与"还没输过"要分开说，人才知道该干什么） */
const wrongPassphrase = ref(false);
/** 状态行：已恢复草稿 / 已保存 / 提交失败 */
const status = ref("");
/** 提交摘要，可留空 */
const summary = ref("");

/**
 * 这一版怎么存。
 *
 * 开编辑器时照**这篇当前的保护**填好，所以"什么都不动"就等于照旧；
 * 真改了（或给出口令）就是给这篇换保护，从这一版起照新的粘住。
 */
const perCommit = ref<Policy>({ ...protection.value });

/** 这一篇的口令。只在这次提交要套对称层时用得上；交给后端会话后就不再留着。 */
const passphraseDraft = ref("");

const chosenLabel = computed(() => policyLabel(perCommit.value));

/**
 * 槽位里那份**还没决定要不要**的草稿。
 *
 * 打开编辑器默认看到的是已提交的那一版 —— 草稿是"上次写了一半"，要不要接着写
 * 得人点头。在决定之前自动保存会让路，免得一敲键盘就把它盖掉。
 */
const pendingDraft = ref<Draft | null>(null);

/** 自动保存：停手三秒后把缓冲区写进草稿槽位 */
const AUTOSAVE_DELAY_MS = 3000;
let autosaveTimer: number | undefined;

/**
 * 预览：由**后端**渲染，与阅读视图同一个渲染器 —— 所以预览里的表格、内部链接、
 * 代码高亮与正文逐字一致，不会出现"预览好看、提交后变样"。
 */
const preview = ref("");
/** 预览渲染失败的原因 */
const previewProblem = ref("");
/** 预览那一层：html 注入之后在它上面补高亮与行号 */
const previewEl = ref<HTMLElement | null>(null);

/**
 * 预览一更新（v-html 换完 DOM）就补上高亮与行号 —— 与阅读视图长成同一个样子。
 */
watch(preview, () => {
    void nextTick(() => {
        if (previewEl.value) {
            highlightCode(previewEl.value);
            applyLineNumbers(previewEl.value);
            // 与阅读视图同一套收尾：右键菜单、图片取不到时给说明
            decorateNoteHtml(previewEl.value);
        }
    });
});

watch(codeLineNumbers, () => {
    if (previewEl.value) {
        applyLineNumbers(previewEl.value);
    }
});

/** 预览渲染的定时器：连续输入期间只排最后一次 */
let previewTimer: number | undefined;

async function refreshPreview(text: string) {
    try {
        preview.value = await invoke<string>("render_markdown", {
            markdown: text,
            title: props.title,
        });
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
    void refreshPreview(markdown.value);
}

/**
 * 每栏的最小可读宽度。
 *
 * 低于这个宽度就**上下排列**，而不是硬挤成两条窄栏：一行代码在 260px 里要折好几次，
 * 折过之后比上下排列还难读。300 是"一行十几字符仍能看清"的位置。
 */
const PANE_MIN_WIDTH = 300;
const PANES_GAP = 12;
const STACK_BREAKPOINT = PANE_MIN_WIDTH * 2 + PANES_GAP;
/**
 * 切回来的阈值比切过去的**高一点**（迟滞）。
 *
 * 两个值贴在一起时，一次布局变化（比如竖滚动条出现，占掉十几像素）就能让宽度在阈值两侧
 * 来回跳，于是"偶尔莫名其妙变成上下排布"。留出这段差量，来回都需要真正跨过一段距离。
 */
const UNSTACK_BREAKPOINT = STACK_BREAKPOINT + 40;

/** 并排时单栏的高度上限（视口减去本页固定开销的权宜值） */
const PANE_HEIGHT = "calc(100vh - 240px)";
/** 上下排布时每栏的高度：两者相加仍不超过上面那个值，页面不会被撑长 */
const STACKED_PANE_HEIGHT = "calc((100vh - 240px) / 2)";

const panesEl = ref<HTMLElement | null>(null);
const stacked = ref(false);

function measurePanes() {
    const el = panesEl.value;
    if (!el) {
        return;
    }
    // 量两栏容器自己是对的：它宽度由父级决定（块级 flex 撑满），**不随排布方向变化**，
    // 所以不会出现"一变成上下排布、可用宽度也跟着变小，于是再也切不回来"的自反馈。
    const width = el.clientWidth;
    // 宽度为 0（还没布局 / 不可见）时不下结论，免得一上来就误判
    if (width <= 0) {
        return;
    }
    stacked.value = stacked.value
        ? width < UNSTACK_BREAKPOINT
        : width < STACK_BREAKPOINT;
}

let panesObserver: ResizeObserver | undefined;

/**
 * 不看旧内容，直接写新的一版。
 *
 * 提交是**追加**一版，所以旧版本一条都不会丢；这条路是给"口令想不起来"用的。
 */
function writeAnyway() {
    locked.value = false;
    loadProblem.value = "";
    markdown.value = "";
    committedMarkdown.value = "";
    hasNote.value = true;
    status.value = "无法读取原有内容，将以新版本写入；此前版本均会保留。";
}

/** 装载：读笔记，再读它槽位里的草稿 */
async function load() {
    loading.value = true;
    loadProblem.value = "";
    status.value = "";
    hasDraft.value = false;
    hasNote.value = false;
    locked.value = false;
    wrongPassphrase.value = false;
    pendingDraft.value = null;

    try {
        const reading = await invoke<Reading>("read_note", {
            title: props.title,
            reference: null,
        });

        // 上了锁：不硬换地址，给两条路选 —— 去解锁，或者不看旧内容直接写新的一版。
        // 后者是"口令丢了"时的出路：旧版本一条都不会被删，只是这一版看不见而已。
        if (reading.state === "locked") {
            locked.value = true;
            wrongPassphrase.value = reading.wrong_passphrase;
            loadProblem.value = reading.wrong_passphrase
                ? "上次输入的口令不正确，解锁未成功。"
                : "此笔记为加密存储，需先解锁才能查看当前内容。";
            loading.value = false;
            return;
        }

        locked.value = false;
        wrongPassphrase.value = false;

        markdown.value = reading.note.markdown;
        committedMarkdown.value = reading.note.markdown;
        hasNote.value = true;

        // 存法优先沿用**这篇当前的保护**：要换得显式改这一栏，改了从这一版起粘住。
        // 全新的一篇（还没有正文）没有可沿用的，就照仓库默认。
        if (reading.note.rev > 0) {
            perCommit.value = policyFrom(reading.note.protection);
        }
    } catch (error) {
        loadProblem.value = String(error);
        loading.value = false;
        return;
    }

    // 槽位里有草稿就摆出来问一声，**不直接盖上去**：默认打开的是已提交的那一版。
    // 草稿读不出来不该挡住编辑：它只是缓冲区，正文已经在手里。
    try {
        const draft = await invoke<Draft | null>("load_draft", { title: props.title });
        hasDraft.value = draft !== null;
        if (draft && draft.markdown !== markdown.value) {
            pendingDraft.value = draft;
        }
    } catch (error) {
        console.debug("读取草稿失败:", error);
    } finally {
        loading.value = false;
    }
}

/** 写草稿槽位。没改动就不写 —— 后端也挡得住，这里省一次往返 */
async function saveDraft() {
    if (!hasNote.value || busy.value || loading.value) {
        return;
    }
    // 有一份草稿还没决定要不要：这期间不动槽位，否则一敲键盘就把它盖掉了
    if (pendingDraft.value) {
        return;
    }
    if (markdown.value === committedMarkdown.value) {
        return;
    }

    try {
        await invoke("save_draft", { title: props.title, markdown: markdown.value });
        hasDraft.value = true;
        status.value = `已自动保存为草稿（${new Date().toLocaleTimeString()}）`;
    } catch (error) {
        status.value = `草稿保存失败：${String(error)}`;
    }
}

/** 提交。成功后回到这篇的阅读地址（由上层改地址） */
async function commit() {
    if (!hasNote.value) {
        return;
    }
    window.clearTimeout(autosaveTimer);
    busy.value = true;
    try {
        const committed = await invoke<Note>("commit_note", {
            title: props.title,
            markdown: markdown.value,
            summary: summary.value.trim() || null,
            protection: perCommit.value,
            // 口令跟着这次写入记给新版本；它只活在这次会话里
            passphrase: perCommit.value.symmetric ? passphraseDraft.value || null : null,
        });
        passphraseDraft.value = "";
        status.value = "";
        emit("navigate", committed.title);
    } catch (error) {
        // 提交失败时不动正在编辑的内容，只把原因写在状态行
        status.value = `提交失败：${String(error)}`;
    } finally {
        busy.value = false;
    }
}

/** 恢复槽位里那份草稿：把人写了一半的东西放回编辑器 */
function restoreDraft() {
    const draft = pendingDraft.value;
    if (!draft) {
        return;
    }
    pendingDraft.value = null;
    markdown.value = draft.markdown;
    status.value = `已恢复未提交的草稿（${draft.modified}）`;
}

/** 不要那份草稿：清掉槽位，编辑器留在已提交的这一版 */
async function discardPending() {
    pendingDraft.value = null;
    await discard();
}

/** 放弃草稿：清掉槽位，回到上一次提交的内容 */
async function discard() {
    if (!hasNote.value) {
        return;
    }
    window.clearTimeout(autosaveTimer);
    busy.value = true;
    try {
        await invoke<boolean>("discard_draft", { title: props.title });
        hasDraft.value = false;
        markdown.value = committedMarkdown.value;
        status.value = "草稿已丢弃，已恢复为上次提交的内容";
    } catch (error) {
        status.value = `丢弃失败：${String(error)}`;
    } finally {
        busy.value = false;
    }
}

/**
 * 上传一个附件，并在光标处插入对它的引用。
 *
 * 路径交给后端去读（字节不经过前端）；插进去的是**引用**（`![](名字)`），
 * 不是地址 —— 笔记里写的始终是名字。
 */
async function insertFile() {
  const picked = await open({ multiple: true, title: "选择要插入的文件" });
  if (!picked) {
    return;
  }
  const paths = Array.isArray(picked) ? picked : [picked];

  busy.value = true;
  try {
    const references: string[] = [];
    for (const path of paths) {
      const uploaded = await invoke<Uploaded>("upload_file", { path });
      references.push(fileReferenceOf(uploaded.entry));
    }
    insertAtCursor(references.join("\n"));
    status.value = `已插入 ${references.length} 个附件`;
  } catch (error) {
    status.value = `插入失败：${String(error)}`;
  } finally {
    busy.value = false;
  }
}

/** 把一段文字插到光标处（没有光标就插到末尾） */
function insertAtCursor(text: string) {
  if (!view) {
    markdown.value += text;
    return;
  }
  const range = view.state.selection.main;
  view.dispatch({
    changes: { from: range.from, to: range.to, insert: text },
    selection: { anchor: range.from + text.length },
  });
  view.focus();
}

/** 编辑器里按 Ctrl+V：剪贴板里是文件就收进来并插入引用 */
async function onPasteFiles(event: ClipboardEvent): Promise<boolean> {
  const picked = clipboardFiles(event);
  if (picked.length === 0) {
    return false;
  }

  busy.value = true;
  try {
    const references: string[] = [];
    await uploadPasted(picked, async (bytes, name) => {
      const uploaded = await invoke<Uploaded>("upload_bytes", bytes, {
        headers: { "x-file-name": encodeURIComponent(name) },
      });
      references.push(fileReferenceOf(uploaded.entry));
    });
    insertAtCursor(references.join("\n"));
    status.value = `已插入 ${references.length} 个附件`;
  } catch (error) {
    status.value = `粘贴上传失败：${String(error)}`;
  } finally {
    busy.value = false;
  }
  return true;
}

/** 退出编辑但**保留**草稿：回到这篇的阅读地址 */
function leave() {
    emit("navigate", props.title);
}

/** CodeMirror 挂载点 */
const hostEl = ref<HTMLElement | null>(null);
let view: EditorView | null = null;

/** 正在把外部改动同步进 CM6 —— 这类改动不触发自动保存 */
let syncing = false;

onMounted(async () => {
    await load();
    if (hostEl.value && hasNote.value) {
        view = new EditorView({
            parent: hostEl.value,
            state: EditorState.create({
                doc: markdown.value,
                extensions: [
                    basicSetup,
                    // 共用那一套排在 basicSetup 后面：主题与高亮要能盖掉它的浅色默认值
                    ...sourceExtensions(),
                    EditorView.lineWrapping,
                    // 剪贴板里是文件（截图、复制的图）就收进仓库并插入引用；
                    // 普通文字返回 false，交回编辑器自己处理
                    EditorView.domEventHandlers({
                        paste: (event) => {
                            void onPasteFiles(event);
                            return false;
                        },
                    }),
                    EditorView.updateListener.of((update) => {
                        if (!update.docChanged || syncing) {
                            return;
                        }
                        markdown.value = update.state.doc.toString();
                        // 输入后延迟自动保存，连续敲字不会每次都写
                        window.clearTimeout(autosaveTimer);
                        autosaveTimer = window.setTimeout(() => void saveDraft(), AUTOSAVE_DELAY_MS);
                    }),
                ],
            }),
        });
        void refreshPreview(markdown.value);
    }

    measurePanes();
    // 挂载那一刻的宽度未必是最终宽度（滚动条、版心过渡、窗口管理器的初始摆放都可能插一脚），
    // 所以下一帧再量一次：迟滞判定只在真正跨过阈值时才改变结论，重测是安全的。
    requestAnimationFrame(measurePanes);

    if (typeof ResizeObserver !== "undefined" && panesEl.value) {
        panesObserver = new ResizeObserver(measurePanes);
        panesObserver.observe(panesEl.value);
    }
    // 窗口变化一律补测一次，不只在没有 ResizeObserver 时
    window.addEventListener("resize", measurePanes);
});

onBeforeUnmount(() => {
    window.clearTimeout(autosaveTimer);
    window.clearTimeout(previewTimer);
    panesObserver?.disconnect();
    window.removeEventListener("resize", measurePanes);
    view?.destroy();
    view = null;
});

// 内容变了就同步编辑器与预览 —— 编辑器只看文档是否一致，预览则重新排队渲染。
// 判等是必需的：否则每个按键都会把内容重设一遍，光标会被打回开头。
watch(markdown, (value) => {
    if (view && value !== view.state.doc.toString()) {
        syncing = true;
        view.dispatch({
            changes: { from: 0, to: view.state.doc.length, insert: value },
        });
        syncing = false;
    }
    schedulePreview(value);
});
</script>

<template>
  <section class="editor">
    <header class="editor__head">
      <h1 class="editor__title">{{ title }}</h1>
    </header>

    <div class="editor__bar">
      <input
          v-model="summary"
          class="editor__summary"
          type="text"
          placeholder="提交说明（可留空）"
          @keydown.enter.prevent="commit"
      />

      <div class="editor__actions">
        <details class="econf">
          <summary class="ecap" title="本版的存储方式；修改后此笔记将沿用新的方式">
            存储：{{ chosenLabel }}
          </summary>

          <div class="econf__body">
            <label class="econf__check">
              <input v-model="perCommit.compress" type="checkbox"/>
              压缩
            </label>

            <label class="econf__field">
              签名密钥
              <input
                  v-model="perCommit.gpg_sign"
                  type="text"
                  placeholder="留空表示不签名"
                  :disabled="!gpgAvailable"
              />
            </label>

            <label class="econf__field">
              加密密钥
              <input
                  v-model="perCommit.gpg_encrypt"
                  type="text"
                  placeholder="留空表示不加密"
                  :disabled="!gpgAvailable"
              />
            </label>
            <p v-if="!gpgAvailable" class="econf__hint">本机未安装 gpg，签名与加密不可用。</p>

            <label class="econf__check">
              <input v-model="perCommit.symmetric" type="checkbox"/>
              口令加密
              <span class="econf__hint">口令仅用于本次会话，不写入磁盘</span>
            </label>

            <label v-if="perCommit.symmetric" class="econf__field">
              口令
              <input v-model="passphraseDraft" type="password" placeholder="本次会话中使用"/>
            </label>
          </div>
        </details>

        <button
            class="ebtn"
            type="button"
            title="上传文件，并在光标处插入引用（也可以直接 Ctrl+V 粘贴）"
            :disabled="busy || !hasNote"
            @click="insertFile"
        >
          <ImagePlus :size="14" :stroke-width="1.9"/>
          插入文件
        </button>

        <button
            class="ebtn"
            type="button"
            title="不等静默，立刻渲染当前内容"
            @click="refreshPreviewNow"
        >
          刷新预览
        </button>

        <button
            class="ebtn"
            type="button"
            :disabled="busy || !hasNote"
            @click="saveDraft"
        >
          <Save :size="14" :stroke-width="1.9"/>
          保存草稿
        </button>
        <button
            class="ebtn ebtn--primary"
            type="button"
            :disabled="busy || !hasNote"
            @click="commit"
        >
          <Check :size="14" :stroke-width="2.2"/>
          提交
        </button>
        <button
            class="ebtn ebtn--danger"
            type="button"
            :disabled="busy || !hasDraft"
            @click="discard"
        >
          <Trash2 :size="14" :stroke-width="1.9"/>
          放弃草稿
        </button>
        <button class="ebtn" type="button" :disabled="busy" @click="leave">
          <X :size="14" :stroke-width="1.9"/>
          取消
        </button>
      </div>
    </div>

    <!-- 有未提交的草稿：是否继续编辑由此处决定，此期间不覆盖草稿 -->
    <div v-if="pendingDraft" class="editor__draft">
      <span class="editor__draft-text">
        存在一份未提交的草稿（{{ pendingDraft.modified }}）。
        当前显示的是已提交的版本。
      </span>
      <button class="ebtn" type="button" :disabled="busy || loading" @click="restoreDraft">
        <RotateCcw :size="14" :stroke-width="1.9"/>
        恢复草稿
      </button>
      <button class="ebtn" type="button" :disabled="busy || loading" @click="discardPending">
        丢弃草稿
      </button>
    </div>

    <p v-if="loading" class="editor__problem">正在读「{{ title }}」…</p>
    <div v-else-if="loadProblem" class="editor__problem">
      <p>{{ loadProblem }}</p>

      <!-- 读不出来（例如解锁被取消）也要有出路：重试，或者退回阅读页 -->
      <div v-if="!locked" class="editor__problem-actions">
        <button type="button" class="ebtn" @click="load">重试</button>
        <button type="button" class="ebtn" @click="emit('navigate', props.title)">返回阅读页</button>
      </div>

      <div v-if="locked" class="editor__problem-actions">
        <button
            type="button"
            class="ebtn"
            @click="emit('navigate', `${props.title}@unlock`)"
        >
          去解锁
        </button>
        <button type="button" class="ebtn" @click="writeAnyway">直接写新的一版</button>
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

    <!--
      分栏直接写在元素上。样式表层面这两条本来也是并排（后出现的规则是 flex row），
      写成内联是为了排除"被某条更靠后的规则覆盖"这一可能 —— 内联样式只有 !important 能压。
      方向也在这里切换：窗口窄了改上下排布。
    -->
    <div
        ref="panesEl"
        class="editor__panes"
        :style="{
        display: 'flex',
        flexDirection: stacked ? 'column' : 'row',
        alignItems: 'stretch',
        gap: PANES_GAP + 'px',
      }"
    >
      <!-- 左：源码（CodeMirror） -->
      <div
          ref="hostEl"
          class="editor__source selectable"
          :style="
          stacked
            ? {
                flex: '0 0 auto',
                height: STACKED_PANE_HEIGHT,
                minWidth: 0,
                overflow: 'hidden',
              }
            : { flex: '1 1 0', minWidth: 0, overflow: 'hidden' }
        "
      />

      <!-- 右：渲染预览（后端同一个渲染器；.note-body 复用正文样式） -->
      <!-- 上下排布时给**确定的高度**：这个组件的高度链不可靠，靠 flex 均分会让 CM6 的滚动容器算不出可视范围 -->
      <div
          class="editor__preview selectable"
          :style="
          stacked
            ? {
                flex: '0 0 auto',
                height: STACKED_PANE_HEIGHT,
                minWidth: 0,
                overflow: 'auto',
              }
            : {
                flex: '1 1 0',
                minWidth: 0,
                overflow: 'auto',
                maxHeight: PANE_HEIGHT,
              }
        "
      >
        <!--
          阅读页那一行大标题。预览的意义就是"看出这一页长什么样"，缺了标题就不像。
        -->
        <h1
            class="preview-title"
            :style="{
            margin: '0',
            padding: '18px 0 12px',
            fontSize: '2.15em',
            fontWeight: 600,
            lineHeight: 1.5,
            overflowWrap: 'anywhere',
          }"
        >
          {{ props.title }}
        </h1>

        <p v-if="previewProblem" class="editor__preview-error">
          预览生成失败：{{ previewProblem }}
        </p>
        <div v-else ref="previewEl" class="note-body" v-html="preview"/>
      </div>
    </div>

    <p class="editor__status">
      <span class="editor__message">{{ status }}</span>
      <!-- 字符数靠状态行右侧 -->
      <span class="editor__meta">
        <span>{{ markdown.length }} 字符</span>
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
  border-color: var(--link-missing);
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

/* ---------- 源码 / 预览 两栏 ---------- */
.editor__panes {
  display: flex;
  flex-direction: row;
  width: 100%;
  align-items: stretch;
  gap: 12px;
  min-height: 320px;
}

.editor__source,
.editor__preview {
  min-width: 0;
  border: 1px solid var(--border);
  border-radius: 8px;
  /*
   * 最高高度直接写在这里，**不依赖祖先链**：只要两栏各有上限，它们就是各自的滚动容器。
   * 数值按视口减去本页固定开销（标题栏 + 编辑栏 + 留白）估的，是权宜值。
   */
  max-height: calc(100vh - 240px);
}

/* 左栏自己不滚：CM6 的虚拟渲染要求它的 `.cm-scroller` 是滚动容器，
   滚外层会让它算错可视范围（内容可能不渲染）。所以外层隐藏溢出，滚动交给它。 */
.editor__source {
  overflow: hidden;
  background: var(--field-bg);
}

.editor__preview {
  overflow: auto;
  padding: 0 14px;
  background: var(--surface);
}

/*
 * CodeMirror 撑满左栏。
 *
 * `.cm-*` 是它自己用 JS 插进来的元素，**不带本组件的 scoped 属性**，所以普通后代选择器
 * 选不到。要穿透作用域，必须用 :deep()。
 */
.editor__source :deep(.cm-editor) {
  /* 高度由内容与上限共同决定，撑满反而会与上限打架 */
  height: auto;
}

.editor__source :deep(.cm-scroller) {
  /* CM6 的滚动容器：最高高度加在它身上，滚动由它负责 */
  max-height: calc(100vh - 240px);
  overflow: auto;
  font-family: var(--mono-font);
  font-size: 13px;
  line-height: 1.7;
}

.editor__preview-error {
  color: var(--link-missing);
  font-size: 13px;
}

/* 「存储」这一项：折叠起来的完整配置。默认值来自这篇当前的保护，这里改只影响这一版起 */
.econf {
  position: relative;
}

.ecap {
  list-style: none;
  display: inline-flex;
  gap: 6px;
  align-items: center;
  padding: 5px 10px;
  border: 1px solid var(--border);
  border-radius: 6px;
  color: var(--text-dim);
  font-size: 12.5px;
  white-space: nowrap;
  cursor: pointer;
}

.ecap::-webkit-details-marker {
  display: none;
}

.econf[open] .ecap {
  border-color: var(--accent-soft);
  color: var(--text);
}

.econf__body {
  position: absolute;
  z-index: 5;
  top: calc(100% + 6px);
  right: 0;
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-width: 300px;
  padding: 12px;
  border: 1px solid var(--border);
  border-radius: 8px;
  background: var(--surface);
  box-shadow: 0 8px 24px rgba(0, 0, 0, 0.28);
  font-size: 12.5px;
}

.econf__check,
.econf__field {
  display: flex;
  gap: 8px;
  align-items: center;
  color: var(--text-dim);
}

.econf__field input {
  flex: 1;
  min-width: 0;
  padding: 5px 8px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: var(--bg);
  color: var(--text);
  font: inherit;
  font-size: 12.5px;
}

.econf__hint {
  color: var(--text-dim);
  font-size: 11.5px;
}

/* 草稿待定条：贴在编辑区上面，把"要不要接着写"摆明 */
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
