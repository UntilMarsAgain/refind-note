<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { Check, Pencil, Save, Trash2, X } from "@lucide/vue";
import type { FileEntry } from "../bindings";
import { checkTitle } from "../title";
import { themeMode } from "../theme";
import { applyLineNumbers, highlightCode } from "../code-blocks";
import { fileReferenceOf } from "../file-links";
import { decorateNoteHtml } from "../note-html";
import { clipboardFiles } from "../paste-files";
import { uploadPasted } from "../paste-upload";
import { open } from "@tauri-apps/plugin-dialog";
import {
  templateBlockLines,
  templateFoldRange,
  templateRanges,
} from "../template-blocks";
import StatePanel from "./StatePanel.vue";
import { codeLineNumbers } from "../settings";
// `codemirror` 是元包（提供 basicSetup 等），EditorState 由 @codemirror/state 提供 ——
// 后者必须作为**直接依赖**安装：pnpm 的严格 node_modules 下，传递依赖不可直接导入。
import { basicSetup } from "codemirror";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { markdown } from "@codemirror/lang-markdown";
import { css as cssLanguage } from "@codemirror/lang-css";
import { html as htmlLanguage } from "@codemirror/lang-html";
import { HighlightStyle, foldService, syntaxHighlighting } from "@codemirror/language";
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
  /** 后端给出的语言（`css` / `html`）；null = markdown。判定只在后端一处 */
  language: string | null;
  /** 当前地址：「状态」面板把它交给后端，用于语言判定与渲染报告 */
  address: string;
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
    /* 限高加在 CM6 自己的滚动容器上：滚外层会让它的虚拟渲染算错可视范围 */
    ".cm-editor": { height: "auto" },
    ".cm-scroller": { maxHeight: "calc(100vh - 240px)", overflow: "auto" },
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
    // 模板块：头行看得出是"一次模板调用"，块内的行淡染一层，边界一目了然
    ".cm-template-head": {
      color: "var(--syntax-keyword)",
      fontWeight: "600",
    },
    ".cm-template-body": {
      // 只盖住行首缩进：那是"这段属于这个块"的标记，别染到正文
      backgroundColor: "var(--accent-tint)",
    },
    // 块的左边缘（整行装饰）：空行也在内，所以跨空行是**连续**的一条
    ".cm-template-line": {
      boxShadow: "inset 3px 0 0 0 var(--accent-tint)",
    },
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

/**
 * 与后端渲染器对齐的 markdown 解析。
 *
 * 后端关掉了两种写法，编辑器这边必须跟着关，否则**高亮会说谎**：
 *
 * - Setext 标题（`标题` 下一行写 `===` 或 `---`）：后端已不再把它解析成标题；
 * - 缩进代码块（四空格开头）：那个缩进后端留给了**模板块的边界**。
 *
 * 不关的后果很具体：编辑器把一段文字画成标题或代码，渲染出来却是普通段落。
 */
const syncedMarkdown = () =>
  markdown({ extensions: [{ remove: ["SetextHeading", "IndentedCode"] }] });

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

  // ---- 代码语言（CSS / HTML）的 token ----
  // 上面那几条只覆盖 markdown 的 token。CM6 编译 CSS / HTML 时会产出完全另一套 tag
  // （标签名、属性名、属性值、注释…），一条都没配色 —— 于是"语言配上了却没有颜色"，
  // 看起来就像语言没生效。这里按同一套 `--syntax-*` 变量补齐。
  { tag: tags.comment, color: "var(--syntax-comment)", fontStyle: "italic" },
  { tag: tags.string, color: "var(--syntax-string)" },
  { tag: tags.number, color: "var(--syntax-number)" },
  { tag: [tags.bool, tags.null], color: "var(--syntax-number)" },
  { tag: tags.keyword, color: "var(--syntax-keyword)" },
  { tag: [tags.controlKeyword, tags.operatorKeyword, tags.definitionKeyword, tags.modifier],
    color: "var(--syntax-keyword)" },
  // 标签名读起来像关键字；属性名像标题
  { tag: tags.tagName, color: "var(--syntax-keyword)" },
  { tag: tags.attributeName, color: "var(--syntax-title)" },
  { tag: tags.propertyName, color: "var(--syntax-title)" },
  { tag: tags.attributeValue, color: "var(--syntax-string)" },
  { tag: [tags.className, tags.typeName], color: "var(--syntax-type)" },
  { tag: [tags.atom, tags.constant(tags.variableName)], color: "var(--syntax-number)" },
  // 括号、运算符之类是结构，压低存在感，别和内容抢
  { tag: [tags.operator, tags.punctuation, tags.bracket, tags.separator, tags.angleBracket],
    color: "var(--text-dim)" },
  { tag: tags.invalid, color: "var(--syntax-deleted)" },
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

const templateHighlight = ViewPlugin.fromClass(
  class {
    decorations: DecorationSet;
    constructor(view: EditorView) {
      this.decorations = buildTemplateDecorations(view);
    }
    update(update: ViewUpdate) {
      if (update.docChanged || update.viewportChanged) {
        this.decorations = buildTemplateDecorations(update.view);
      }
    }
  },
  { decorations: (plugin) => plugin.decorations },
);

/**
 * 模板块的高亮。
 *
 * CM6 的 markdown 语法不认识 `::名字`（本项目的扩展语法），所以在**视图层**加装饰。
 * **算什么装饰在 `template-blocks.ts`**（纯函数，与后端逐条一致，并有用例）；
 * 这里只负责把"行号 + 列"换成 CM6 的文档偏移量。
 */
function buildTemplateDecorations(view: EditorView): DecorationSet {
  // 算什么、落在哪里，全在纯函数里（有用例）；这里只剩最后一步：变成装饰
  const doc = view.state.doc;
  const lines = doc.toString().split("\n");
  const items: { from: number; to: number; decoration: Decoration }[] = [];

  // 标记：整行（头行）或行首缩进（块内）
  for (const range of templateRanges(lines)) {
    items.push({
      from: range.from,
      to: range.to,
      decoration: Decoration.mark({
        class: range.head ? "cm-template-head" : "cm-template-body",
      }),
    });
  }

  // 整行装饰：块的左边缘。**空行也要** —— 空行没有列可以上色，
  // 只靠标记的话块会在空行处断开一条缝，看上去像"没跨过空行"（其实跨过了）。
  for (const index of templateBlockLines(lines)) {
    const line = doc.line(index + 1);
    items.push({
      from: line.from,
      to: line.from,
      decoration: Decoration.line({ class: "cm-template-line" }),
    });
  }

  items.sort((a, b) => a.from - b.from || a.to - b.to);
  return Decoration.set(
    items.map((item) => item.decoration.range(item.from, item.to)),
    true,
  );
}



/**
 * 模板块的折叠。
 *
 * CM6 默认按 markdown 的结构折（段落、标题…），而模板块对它只是一段普通文字 ——
 * 折到第一个空行就停了，与渲染的"跨空行"对不上。这里按**同一套规则**给出范围：
 * 头行到块的最后一行（含块内空行）。
 *
 * 折叠服务优先于语法树自带的折叠属性，所以这里的结论会盖过默认行为。
 */
const templateFold = foldService.of((state, lineStart) => {
  const doc = state.doc;
  const line = doc.lineAt(lineStart);
  const range = templateFoldRange(doc.toString().split("\n"), line.number - 1);
  if (!range) {
    return null;
  }
  return { from: line.to, to: doc.line(range.to + 1).to };
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
/**
 * 光标与选区。
 *
 * 只有编辑器自己知道（CM6 的状态不在 props 里），所以在这里算好交给「状态」面板 ——
 * "我想确认光标到底在第几行"这类问题不必靠数。
 */
const cursorText = ref("第 1 行，第 1 列");
const selectionText = ref("未选中");

function updateCursor(state: EditorState) {
  const range = state.selection.main;
  const head = state.doc.lineAt(range.head);
  cursorText.value =
    "第 " + head.number + " 行，第 " + (range.head - head.from + 1) + " 列";
  const selected = range.to - range.from;
  if (selected === 0) {
    selectionText.value = "未选中";
    return;
  }
  const anchor = state.doc.lineAt(range.anchor);
  selectionText.value =
    "选中 " + selected + " 字符，跨 " +
    (Math.abs(anchor.number - head.number) + 1) + " 行";
}

/** 预览那一层（行号加在它上面；预览故意不做高亮：每次输入都会重跑） */
const previewEl = ref<HTMLElement | null>(null);

/**
 * 预览一更新（v-html 换完 DOM）就补上高亮与行号。
 *
 * 预览与阅读视图走**同一个渲染器**，所以这里也该长成同一个样子：以前只加行号，
 * 代码是纯色的一片，看不出"编译出来是什么样"。
 */
watch(preview, () => {
  void nextTick(() => {
    if (previewEl.value) {
      highlightCode(previewEl.value);
      applyLineNumbers(previewEl.value);
      // 与阅读视图同一套收尾：附件地址解析、点击看大图、图片取不到时给说明
      decorateNoteHtml(previewEl.value);
    }
  });
});

watch(codeLineNumbers, () => {
  if (previewEl.value) {
    applyLineNumbers(previewEl.value);
  }
});
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

/**
 * 窄窗口时两栏改为上下排布。
 *
 * 判据是**两栏容器自己的宽度**，不是窗口宽度：左侧标签栏展开/收起、外层留白都会改变
 * 可用空间，量窗口会得出错误结论。
 *
 * 为什么连"方向"都写在内联样式里：这个组件里 scoped CSS 在当前 WebView 下不生效
 * （当初两栏不能独立滚动就是栽在这上面），所以布局只能写在元素上。
 */
/**
 * 一栏最窄多少还算"能看"。
 *
 * 阈值**由它推出来**，而不是拍一个总数：两栏各占一半，所以
 * `2 × 最窄 + 间距` 就是该并排的最小宽度。以前写死 720，等于要求每栏 354px ——
 * 窗口一窄就翻成上下排布，而且再也回不去（那其实不是判定错，是门槛太高：
 * 可用宽度 = 窗口 ÷ 基准缩放 − 标签栏 − 正文内边距，还会被阅读栏上限再夹一次）。
 */
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

onMounted(() => {
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
  panesObserver?.disconnect();
  window.removeEventListener("resize", measurePanes);
});

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
        // 语言按后端判定选：模板命名空间里的 .css / .html 用各自语言，其余 markdown
        props.language === "css"
          ? cssLanguage()
          : props.language === "html"
            ? htmlLanguage()
            : syncedMarkdown(),
        // 顺序有讲究：主题与高亮都要排在 basicSetup **之后**，才能盖掉它的浅色默认值
        appTheme,
        syntaxHighlighting(appHighlight),
        wikilinkHighlight,
        templateHighlight,
        templateFold,
        EditorView.lineWrapping,
        /**
         * 粘贴上传：截图或复制的图片直接进仓库，并在光标处写好引用。
         *
         * 只接管"剪贴板里有文件"的情况；普通文字粘贴返回 `false`，照旧交给编辑器自己。
         */
        EditorView.domEventHandlers({
          paste(event, view) {
            const files = clipboardFiles(event);
            if (files.length === 0) {
              return false;
            }
            event.preventDefault();
            fileProblem.value = null;
            void uploadPasted(files)
              .then((entries) => {
                const text = entries.map((entry) => fileReferenceOf(entry)).join("\n");
                view.dispatch(view.state.replaceSelection(text));
                view.focus();
              })
              .catch((error) => {
                fileProblem.value = String(error);
              });
            return true;
          },
        }),
        EditorView.updateListener.of((update) => {
          // 光标与选区：状态面板要报，而 CM6 只有编辑器自己知道
          if (update.selectionSet || update.docChanged) {
            updateCursor(update.state);
          }
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

  if (view) {
    updateCursor(view.state);
  }
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

/**
 * 识别到的语言（markdown 是默认值，不显示）。
 *
 * 显示出来是为了**能一眼验证**：判定发生在后端，这里只把结果显示出来；
 * 打开一个 `template:` 下的 `.css` / `.html` 页面，状态行里应当出现对应的语言。
 */
const languageLabel = computed(() => {
  switch (props.language) {
    case "css":
      return "CSS";
    case "html":
      return "HTML";
    default:
      return "";
  }
});

/** 词法问题（空、@、非法字符、过长）即时反馈，不打扰后端 */
const renameProblem = ref<string | null>(null);
/** 上传/插入文件时出的问题（与改名的问题分开，各自说各自的事） */
const fileProblem = ref<string | null>(null);

/**
 * 快速上传：选文件 → 收进仓库 → 在光标处插入引用。
 *
 * 引用写的是**文件名**（`![名字](名字)`），因为磁盘上的标识是生成的、人记不住；
 * 名字到取件地址的换算在 `decorateNoteHtml` 里统一做。
 */
async function insertFile() {
  fileProblem.value = null;
  try {
    const picked = await open({ multiple: true, title: "选择要插入的文件" });
    const paths = Array.isArray(picked) ? picked : picked ? [picked] : [];
    if (paths.length === 0) {
      return;
    }
    const references: string[] = [];
    for (const path of paths) {
      const entry = await invoke<FileEntry>("upload_file", { path });
      references.push(fileReferenceOf(entry));
    }
    view?.dispatch(view.state.replaceSelection(references.join("\n")));
    view?.focus();
  } catch (error) {
    fileProblem.value = String(error);
  }
}

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
      <span v-if="fileProblem" class="editor__problem">{{ fileProblem }}</span>
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
          class="ebtn"
          type="button"
          title="上传文件，并在光标处插入引用（也可以直接 Ctrl+V 粘贴）"
          @click="insertFile"
        >
          <ImagePlus :size="14" :stroke-width="1.9" />
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
      <!-- 上下排布时给**确定的高度**：这个组件的高度链不可靠（见上），
           靠 flex 均分会让 CM6 的滚动容器算不出可视范围 -->
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
          用 props 里的当前标题，而不是改名输入框里那份草稿：改名是另一次动作，
          还没生效就不该在预览里提前显示。
          尺寸与阅读页的 .page-title 保持一致（2.15em / 600 / 1.5）。
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
        <div v-else ref="previewEl" class="note-body" v-html="preview" />
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
      <!-- 字符数与语言并成一组：状态行两端对齐，多一个孤立元素会被摊到中间 -->
      <span class="editor__meta">
        <span>{{ modelValue.length }} 字符</span>
        <span v-if="languageLabel" class="editor__language">{{ languageLabel }}</span>
      </span>
    </p>

    <!--
      状态面板：编辑页当前的事实（编辑状态 / 预览编译 / 布局实测）。
      放在编辑器下方，因为它说的是**这一页这一次编辑**的状态。
    -->
    <StatePanel
      :title="title"
      :address="address"
      :markdown="modelValue"
      :language="language"
      :status="status"
      :cursor="cursorText"
      :selection="selectionText"
    />

  </section>
</template>

<style scoped>
/*
 * 根撑满父级高度。
 *
 * 这里**不能用绝对定位**：编辑器外面还有一层阅读栏（它同时提供宽度限制，并且是高度链的
 * 一环），绝对定位会把它整个绕过 —— 既丢掉宽度限制，也丢掉高度传递，两栏就退回"和页面
 * 共用一个滚动条"。正确做法是让那一层参与进来，高度沿链逐级传（规则在 App.vue 里，用
 * :has(.editor) 只对编辑页生效）。
 */
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

/* 字符数与识别到的语言：同一组，靠状态行右侧 */
.editor__meta {
  display: flex;
  align-items: center;
  gap: 8px;
}

.editor__language {
  padding: 0 6px;
  border: 1px solid var(--border);
  border-radius: 4px;
  color: var(--accent);
  font-size: 0.92em;
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
}

.editor__preview {
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
  /* 不再要求撑满外层：外层高度由内容与上限共同决定，撑满反而会与上限打架 */
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
 * 两栏各占剩余高度、各自滚动。
 *
 * 关键不是加 overflow，而是**高度要有确定来源**：根 `.editor` 已绝对定位为定高，
 * `flex: 1 1 auto` + `min-height: 0` 让两栏吃掉剩余空间，再各自 `overflow: auto`。
 * 中间没有任何一环由内容撑开，所以滚轮滚的必然是所在那一栏。
 */
.editor__panes {
  /* 基准必须是 0，不能是 auto：auto 的基准是**内容高度**，内容一高，两栏就被撑开、
     再被容器裁掉 —— 表现就是"没有滚动条、底部被截断"。基准 0 时高度完全由可用空间决定。 */
  flex: 1 1 0;
  height: auto;
  min-height: 0;
}

/* 状态为空时也占住这一行，避免布局上下跳 */
  content: "　";
}
</style>
