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
 * 把附件收进仓库，并在光标处插一句对它的引用。
 *
 * 为什么与编辑状态分开：这一块的动作是"上传 + 插入"，落盘走 `upload_file` /
 * `upload_bytes`，插进去的是**引用**（`![](名字)`），不是地址 —— 笔记里写的始终是名字
 * （那条规矩在 `dom/file-links.ts` 里，两处共用同一个 `fileReferenceOf`）。
 *
 * 剪贴板那一半只借 `dom/paste-files.ts`：从粘贴事件里捞文件、给临时名起个像样的名字、
 * 读成字节 —— 那边已经有了，这里不重复实现。
 *
 * 它要的东西只有两样：一个能插入的编辑器，和"说一句结果"的通道（状态行 + 忙标记）。
 */

import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import type { Ref } from "vue";
import type { EditorView } from "@codemirror/view";
import { fileReferenceOf } from "../dom/file-links.ts";
import { clipboardFiles, uploadPasted } from "../dom/paste-files.ts";
import type { Uploaded } from "../ipc/files.ts";

export interface AttachmentInsertHost {
    /** CodeMirror 实例：插到光标处用；还没有就退回到"追加到末尾" */
    view: Ref<EditorView | null>;
    /** 编辑器里的源码。没有编辑器时（笔记没装载成功）插进来的字靠它落到界面上 */
    markdown: Ref<string>;
    /** 状态行：说一句"插了几个"或者失败原因 */
    status: Ref<string>;
    /** 忙标记：上传期间禁掉按钮 */
    busy: Ref<boolean>;
}

export interface AttachmentInsert {
    /** 「插入文件」：先选文件，再上传，再插入引用 */
    insertFile: () => Promise<void>;
    /** 编辑器里按 Ctrl+V：剪贴板里是文件就收进来并插入引用 */
    onPasteFiles: (event: ClipboardEvent) => Promise<boolean>;
}

export function useAttachmentInsert(host: AttachmentInsertHost): AttachmentInsert {
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

        host.busy.value = true;
        try {
            const references: string[] = [];
            for (const path of paths) {
                const uploaded = await invoke<Uploaded>("upload_file", { path });
                references.push(fileReferenceOf(uploaded.entry));
            }
            insertAtCursor(references.join("\n"));
            host.status.value = `已插入 ${references.length} 个附件`;
        } catch (error) {
            host.status.value = `插入失败：${String(error)}`;
        } finally {
            host.busy.value = false;
        }
    }

    /** 把一段文字插到光标处（没有光标就插到末尾） */
    function insertAtCursor(text: string) {
        const editor = host.view.value;
        if (!editor) {
            host.markdown.value += text;
            return;
        }
        const range = editor.state.selection.main;
        editor.dispatch({
            changes: { from: range.from, to: range.to, insert: text },
            selection: { anchor: range.from + text.length },
        });
        editor.focus();
    }

    /** 编辑器里按 Ctrl+V：剪贴板里是文件就收进来并插入引用 */
    async function onPasteFiles(event: ClipboardEvent): Promise<boolean> {
        const picked = clipboardFiles(event);
        if (picked.length === 0) {
            return false;
        }

        host.busy.value = true;
        try {
            const references: string[] = [];
            await uploadPasted(picked, async (bytes, name) => {
                const uploaded = await invoke<Uploaded>("upload_bytes", bytes, {
                    headers: { "x-file-name": encodeURIComponent(name) },
                });
                references.push(fileReferenceOf(uploaded.entry));
            });
            insertAtCursor(references.join("\n"));
            host.status.value = `已插入 ${references.length} 个附件`;
        } catch (error) {
            host.status.value = `粘贴上传失败：${String(error)}`;
        } finally {
            host.busy.value = false;
        }
        return true;
    }

    return { insertFile, onPasteFiles };
}