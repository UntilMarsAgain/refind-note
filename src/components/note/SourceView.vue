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
import { onBeforeUnmount, onMounted, ref, watch } from "vue";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { basicSetup } from "codemirror";
import {
    registerEditorView,
    sourceExtensions,
    unregisterEditorView,
} from "../../dom/editor-setup.ts";

/**
 * **只看不改**的源码视图。
 *
 * 与编辑器用的是同一套 CodeMirror 配置（`dom/editor-setup.ts`）：配色、模板块的
 * 装饰、`[[内部链接]]`、代码块的语言高亮都一模一样。差别只有一条 —— 这里是只读的：
 * `editable: false` 让内容不再是可编辑区（光标不进、字敲不进去），
 * `readOnly: true` 再挡住快捷键与命令这一类改文档的路径。
 *
 * 于是"看源码"看到的是**真的源码**（能选、能复制、能折叠、能 Ctrl+F 找），
 * 而不是被当成纯文本摊开的一段字 —— 缩进、模板标记、链接写法都还在原来的位置。
 */
const props = defineProps<{
    /** 要摊开的源码。换了内容（比如从一页帮助换到另一页）当场换掉 */
    markdown: string;
}>();

const hostEl = ref<HTMLElement | null>(null);
let view: EditorView | null = null;

/** 只读的那一份配置：内容换的时候照着重来一遍（见下面的 watch） */
function readonlyState(doc: string): EditorState {
    return EditorState.create({
        doc,
        extensions: [
            basicSetup,
            ...sourceExtensions(),
            EditorView.lineWrapping,
            // 只读：不许编辑，但读得舒服（选区、复制、折叠、查找都在）
            EditorState.readOnly.of(true),
            EditorView.editable.of(false),
        ],
    });
}

onMounted(() => {
    if (!hostEl.value) {
        return;
    }
    view = new EditorView({ parent: hostEl.value, state: readonlyState(props.markdown) });
    // 登记：全局那个 `find` 动作要靠它找到"该对哪个编辑器开面板"
    registerEditorView(view);
});

// 换了一页就整篇换掉：内容与折叠、滚动位置一起归零，看到的是新那一页
watch(
    () => props.markdown,
    (value) => {
        if (!view || value === view.state.doc.toString()) {
            return;
        }
        view.setState(readonlyState(value));
    },
);

onBeforeUnmount(() => {
    if (view) {
        // 先销号：不销号的话登记表里会一直指着这个已经 `destroy()` 的实例
        unregisterEditorView(view);
    }
    view?.destroy();
    view = null;
});
</script>

<template>
    <div ref="hostEl" class="source-view selectable"/>
</template>

<style scoped>
/*
 * 框住它，与正文的其余部分分开。高度不再另加上限：CM6 的 `.cm-scroller` 已经带了
 * 一条（见 `editor-setup.ts` 的主题），这里再加一条会两层打架。
 */
.source-view {
    margin: 14px 0 0;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--field-bg);
    overflow: hidden;
}

/* `.cm-*` 是 CM6 自己插进来的元素，不带本组件的 scoped 属性，得用 :deep() 穿透 */
.source-view :deep(.cm-editor) {
    height: auto;
}

.source-view :deep(.cm-scroller) {
    font-family: var(--mono-font);
    font-size: 13px;
    line-height: 1.75;
}
</style>
