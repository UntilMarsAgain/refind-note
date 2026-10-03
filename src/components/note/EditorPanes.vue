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

<!--
  编辑器的两栏：左边源码、右边预览。

  单独一个组件是因为这两栏是一块**摆放**：方向（并排 / 上下）、每栏多高、滚动归谁，
  都在这里，与上面那排按钮、状态行毫无关系。测量逻辑在 `usePaneLayout.ts`，
  那边量出 `stacked` 交给这里摆。

  它是**纯摆放**：不碰笔记内容，也不碰 CodeMirror 实例 —— 左栏只是那个实例的挂载点
  （`hostEl` 由外面传进来一个 ref），插进来的内容与光标都归外面管。
  预览的 html 由外面传进来（那边管什么时候去渲染），高亮与行号的收尾由这里做，
  因为那要拿到这一栏的 DOM 元素。

  DOM 形状与 class 名一个字没动 —— 外层样式与 `dom/*.ts` 按 class 名找元素。
-->

<script setup lang="ts">
import { computed, nextTick, ref, watch, type CSSProperties, type Ref } from "vue";
import { applyLineNumbers, codeLineNumbers, highlightCode } from "../../dom/code-blocks.ts";
import { decorateNoteHtml } from "../../dom/note-html.ts";
import { PANE_HEIGHT, PANES_GAP, STACKED_PANE_HEIGHT, usePaneLayout } from "../../composables/usePaneLayout.ts";

const props = defineProps<{
    /** 正在编辑的标题。预览里那行大标题就是它 */
    title: string;
    /** 预览 html（由后端渲染，与阅读视图同一个渲染器） */
    preview: string;
    /** 预览渲染失败的原因。非空时这一栏显示它，而不是正文 */
    previewProblem: string;
    /**
     * 左栏的挂载点：一个 `Ref<HTMLElement | null>` 而不是元素本身。
     *
     * 外面（`useCodeMirror.ts`）持有编辑器实例，也就该持有它的挂载点；
     * 传 ref 过去让它在同一个组件里被填上，就不必等 `mounted` 之后再来回问一句。
     */
    hostEl: Ref<HTMLElement | null>;
}>();

// 两栏容器（`panesEl`）：量的是它自己，不是两栏里的任何一栏
const { panesEl, stacked } = usePaneLayout();

/**
 * 把挂到两栏容器上的那个元素收进 ref。
 *
 * 用函数 ref 而不是 `ref="panesEl"`：那个 ref 是从 `usePaneLayout()` 拿来的，
 * 要的就是**它本人**，不是"按名字找到的那个绑定"。
 */
function setPanesEl(el: unknown) {
    panesEl.value = (el as HTMLElement | null) ?? null;
}

/** 预览那一层：html 注入之后在它上面补高亮与行号 */
const previewEl = ref<HTMLElement | null>(null);

/**
 * 预览一更新（v-html 换完 DOM）就补上高亮与行号 —— 与阅读视图长成同一个样子。
 *
 * 公式（`dom/math.ts`）与图表（`dom/diagrams.ts`）的渲染也在 `decorateNoteHtml` 里，
 * 与阅读视图共用同一处收尾，所以两边的公式与图表不会走成两套。
 */
watch(
    () => props.preview,
    () => {
        void nextTick(() => {
            if (previewEl.value) {
                highlightCode(previewEl.value);
                applyLineNumbers(previewEl.value);
                // 与阅读视图同一套收尾：右键菜单、图片取不到时给说明
                decorateNoteHtml(previewEl.value);
            }
        });
    },
);

watch(codeLineNumbers, () => {
    if (previewEl.value) {
        applyLineNumbers(previewEl.value);
    }
});

// 上下排布时给**确定的高度**：这个组件的高度链不可靠，靠 flex 均分会让 CM6 的滚动容器算不出可视范围
const sourceStyle = computed<CSSProperties>(() =>
    stacked.value
        ? {
            flex: "0 0 auto",
            height: STACKED_PANE_HEIGHT,
            minWidth: 0,
            overflow: "hidden",
        }
        : { flex: "1 1 0", minWidth: 0, overflow: "hidden" },
);

const previewStyle = computed<CSSProperties>(() =>
    stacked.value
        ? {
            flex: "0 0 auto",
            height: STACKED_PANE_HEIGHT,
            minWidth: 0,
            overflow: "auto",
        }
        : {
            flex: "1 1 0",
            minWidth: 0,
            overflow: "auto",
            maxHeight: PANE_HEIGHT,
        },
);

// 分栏直接写在元素上。样式表层面这两条本来也是并排（后出现的规则是 flex row），
// 写成内联是为了排除"被某条更靠后的规则覆盖"这一可能 —— 内联样式只有 !important 能压。
const panesStyle = computed<CSSProperties>(() => ({
    display: "flex",
    flexDirection: stacked.value ? "column" : "row",
    alignItems: "stretch",
    gap: `${PANES_GAP}px`,
}));

/**
 * 阅读页那一行大标题。
 *
 * 预览的意义就是"看出这一页长什么样"，缺了标题就不像。排版与 `PageHeader.vue`
 * 的 `.page-title` 对齐（2.15em / 600 / 行高 1.5），两处看着是同一款。
 */
const titleStyle: CSSProperties = {
    margin: "0",
    padding: "18px 0 12px",
    fontSize: "2.15em",
    fontWeight: 600,
    lineHeight: 1.5,
    overflowWrap: "anywhere",
};
</script>

<template>
  <div :ref="setPanesEl" class="editor__panes" :style="panesStyle">
    <!-- 左：源码（CodeMirror） -->
    <div :ref="props.hostEl" class="editor__source selectable" :style="sourceStyle"/>

    <!-- 右：渲染预览（后端同一个渲染器；.note-body 复用正文样式） -->
    <div class="editor__preview selectable" :style="previewStyle">
      <h1 class="preview-title" :style="titleStyle">
        {{ props.title }}
      </h1>

      <p v-if="props.previewProblem" class="editor__preview-error">
        预览生成失败：{{ props.previewProblem }}
      </p>
      <div v-else ref="previewEl" class="note-body" v-html="props.preview"/>
    </div>
  </div>
</template>

<style scoped>
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
</style>