//   Refind Note is a note-taking software.
//   Copyright (C) 2026 Until Mars Again
//
//   This program is free software: you can redistribute it and/or modify
//   it under the terms of the GNU Affero General Public License as published by
//   the Free Software Foundation, either version 3 of the License, or
//   (at your option) any later version.
//
//   This program is distributed in the hope that it will be useful,
//   but WITHOUT ANY WARRANTY; without even the implied warranty of
//   MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
//   GNU Affero General Public License for more details.
//
//   You should have received a copy of the GNU Affero General Public License
//   along with this program.  If not, see <http://www.gnu.org/licenses/>.

/**
 * 源码栏里那个 CodeMirror 6 实例的生命周期。
 *
 * 与编辑状态（`useNoteEditing.ts`）分开，是因为这一边只管**编辑器这个对象**：
 * 把它挂到 DOM 上、把外面的内容推进去、把里面发生的改动报上去、走的时候拆掉。
 * 那边管的是"这一篇笔记"，不该知道 CodeMirror 有一份文档、有一份选区。
 *
 * 扩展（主题、markdown 高亮、公式与图表那些前端渲染）都在 `dom/editor-setup.ts` 里，
 * 这里只负责**组装**：共用那一套排在 `basicSetup` 之后，顺序不能反
 * —— 主题与高亮要能盖掉它的浅色默认值。
 *
 * 销毁由组件在 `onBeforeUnmount` 里**显式**调 `destroy()`，不在这里自己挂钩子：
 * 卸载那几件事有先后（先记状态与落盘，再拆编辑器），顺序由组件说了算，见下面
 * `destroy` 那段说明。
 */

import { ref, type Ref } from "vue";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
// `codemirror` 是元包（提供 basicSetup 等），EditorState 由 @codemirror/state 提供 ——
// 后者必须作为**直接依赖**安装：pnpm 的严格 node_modules 下，传递依赖不可直接导入。
import { basicSetup } from "codemirror";
import {
    registerEditorView,
    sourceExtensions,
    unregisterEditorView,
} from "../dom/editor-setup.ts";

export interface CodeMirrorHost {
    /**
     * 实例放在哪。
     *
     * 由组件持有这一个 ref、两边共用：编辑状态要靠它读光标与滚动（见
     * `useNoteEditing`），共用同一个 ref 才不会各记一份、也不会读到过期的那一份。
     */
    view: Ref<EditorView | null>;
    /** 当前源码（编辑器建起来时用它当初始文档） */
    text: () => string;
    /** 人在编辑器里改了文档（不是外面推进来的） */
    onDocChanged: (text: string) => void;
    /** 光标挪了：切回来时视线要落回原处 */
    onSelectionChanged: () => void;
    /** 编辑器里按 Ctrl+V：剪贴板里是文件就收进来并插入引用 */
    onPaste: (event: ClipboardEvent) => void;
}

export interface CodeMirrorEditor {
    /** CodeMirror 挂载点 */
    hostEl: Ref<HTMLElement | null>;
    /** 建实例。**必须在装载之后**调：初始文档就是装载的结果 */
    mount: () => void;
    /** 外面改了内容：推进编辑器 */
    syncText: (text: string) => void;
    /**
     * 拆掉实例。
     *
     * 由组件在 `onBeforeUnmount` 里**显式**调，不在这一份里自己挂钩子：卸载时的
     * 顺序有讲究 —— 先把编辑状态记下并落盘（`useNoteEditing` 的 `flushOnLeave`），
     * 再拆编辑器，顺序反了记下来的光标就是空的。
     */
    destroy: () => void;
}

export function useCodeMirror(host: CodeMirrorHost): CodeMirrorEditor {
    const view = host.view;
    const hostEl = ref<HTMLElement | null>(null);

    /** 正在把外部改动同步进 CM6 —— 这类改动不触发自动保存 */
    let syncing = false;

    function mount() {
        if (!hostEl.value) {
            return;
        }
        view.value = new EditorView({
            parent: hostEl.value,
            state: EditorState.create({
                doc: host.text(),
                extensions: [
                    basicSetup,
                    // 共用那一套排在 basicSetup 后面：主题与高亮要能盖掉它的浅色默认值
                    ...sourceExtensions(),
                    EditorView.lineWrapping,
                    // 剪贴板里是文件（截图、复制的图）就收进仓库并插入引用；
                    // 普通文字返回 false，交回编辑器自己处理
                    EditorView.domEventHandlers({
                        paste: (event) => {
                            void host.onPaste(event);
                            return false;
                        },
                    }),
                    EditorView.updateListener.of((update) => {
                        // 光标挪了也记一笔：切回来时视线要落回原处
                        if (update.selectionSet) {
                            host.onSelectionChanged();
                        }
                        if (!update.docChanged || syncing) {
                            return;
                        }
                        host.onDocChanged(update.state.doc.toString());
                        host.onSelectionChanged();
                    }),
                ],
            }),
        });
        // 登记：全局那个 `find` 动作要靠它找到"该对哪个编辑器开面板"
        registerEditorView(view.value);
    }

    /**
     * 外面改了内容：把它推进编辑器。
     *
     * 判等是必需的：否则每个按键都会把内容重设一遍，光标会被打回开头。
     */
    function syncText(text: string) {
        const editor = view.value;
        if (!editor || text === editor.state.doc.toString()) {
            return;
        }
        syncing = true;
        editor.dispatch({
            changes: { from: 0, to: editor.state.doc.length, insert: text },
        });
        syncing = false;
    }

    /** 拆掉实例（卸载时由组件在记完状态之后调，见上面 `destroy` 那段说明） */
    function destroy() {
        if (view.value) {
            // 先销号再拆：不销号的话，登记表里会一直指着一个已经 `destroy()` 的
            // 实例，而 `find` 动作拿它开面板就会炸在"对一个死视图 dispatch"上
            unregisterEditorView(view.value);
        }
        view.value?.destroy();
        view.value = null;
    }

    return { hostEl, mount, syncText, destroy };
}