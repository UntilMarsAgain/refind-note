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
import type { Policy } from "../ipc/note.ts";

export interface AttachmentInsertHost {
    /** CodeMirror 实例：插到光标处用；还没有就退回到"追加到末尾" */
    view: Ref<EditorView | null>;
    /** 编辑器里的源码。没有编辑器时（笔记没装载成功）插进来的字靠它落到界面上 */
    markdown: Ref<string>;
    /** 状态行：说一句"插了几个"或者失败原因 */
    status: Ref<string>;
    /** 忙标记：上传期间禁掉按钮 */
    busy: Ref<boolean>;
    /**
     * 这一版笔记的口令（提交那一栏里输的）。
     *
     * 附件进的是**同一个** blob 仓，所以它也可能是加密的；而这里没有单独的口令栏
     * —— 用户刚为这一版输过的那把就是最该用的那把，于是借它。
     * 没给就退回到"照仓库默认"，那是后端本来就有的行为。
     */
    passphrase?: Ref<string>;
    /**
     * 附件这一版**照仓库默认**存（不给 `protection`，后端就按它落）。
     *
     * 要它是因为那句"这里没法输口令"必须说得准：新建的附件页照的就是仓库默认，
     * 而 `add_file` 不给策略时正是照这一页当前的（新页即仓库默认）。
     */
    defaultPolicy?: Ref<Policy>;
}

export interface AttachmentInsert {
    /** 「插入文件」：先选文件，再上传，再插入引用 */
    insertFile: () => Promise<void>;
    /** 编辑器里按 Ctrl+V：剪贴板里是文件就收进来并插入引用 */
    onPasteFiles: (event: ClipboardEvent) => Promise<boolean>;
}

/**
 * 要不要拦下来先说一声，而不是让后端回一句看不懂的错。
 *
 * 后端那句是"这一份是加密的，需要输入口令"——**它是站在读的一侧说的**，
 * 放在上传失败的场合里就成了谜语：用户明明在往里写，怎么要"输入口令"？
 * 所以这里在动手之前就判一次，判得出就直接告诉他去哪儿输。
 *
 * @returns 拦下来时是一句给用户看的话；不用拦是 `null`
 */
export function missingPassphrase(policy: Policy | undefined, typed: string): string | null {
    if (!policy?.symmetric || typed) {
        return null;
    }
    return "仓库默认给内容套了口令加密，而这里没有口令可输：请在上方「口令」那一栏输一把再插入，或到「文件」页上传";
}

/** 上传时递过去的口令：只有真要套口令层、而用户确实输了的时候才给 */
function secretToSend(policy: Policy | undefined, typed: string): string | null {
    return policy?.symmetric && typed ? typed : null;
}

export function useAttachmentInsert(host: AttachmentInsertHost): AttachmentInsert {
    /** 上传之前先判一次"有没有口令可用"，省得传完了才报一句谜语 */
    function passphraseProblem(): string | null {
        return missingPassphrase(host.defaultPolicy?.value, host.passphrase?.value ?? "");
    }

    /**
     * 上传一个附件，并在光标处插入对它的引用。
     *
     * 路径交给后端去读（字节不经过前端）；插进去的是**引用**（`![](名字)`），
     * 不是地址 —— 笔记里写的始终是名字。
     */
    async function insertFile() {
        const problem = passphraseProblem();
        if (problem) {
            host.status.value = problem;
            return;
        }

        const picked = await open({ multiple: true, title: "选择要插入的文件" });
        if (!picked) {
            return;
        }
        const paths = Array.isArray(picked) ? picked : [picked];

        host.busy.value = true;
        try {
            const secret = secretToSend(host.defaultPolicy?.value, host.passphrase?.value ?? "");
            const references: string[] = [];
            for (const path of paths) {
                const uploaded = await invoke<Uploaded>("upload_file", {
                    path,
                    passphrase: secret,
                });
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
        const problem = passphraseProblem();
        if (problem) {
            host.status.value = problem;
            // 仍然算"处理过了"：不这么做的话剪贴板里的文件会连同正文一起被粘贴进来
            return true;
        }

        host.busy.value = true;
        try {
            const secret = secretToSend(host.defaultPolicy?.value, host.passphrase?.value ?? "");
            const references: string[] = [];
            await uploadPasted(picked, async (bytes, name) => {
                // 二进制通道：请求体整个是文件字节，其余参数只能走请求头（后端从头上取）
                const headers: Record<string, string> = {
                    "x-file-name": encodeURIComponent(name),
                };
                if (secret) {
                    headers["x-passphrase"] = encodeURIComponent(secret);
                }
                const uploaded = await invoke<Uploaded>("upload_bytes", bytes, { headers });
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