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
 * 正在编辑的那一篇：源码、装载、草稿、提交与丢弃，以及自动保存的节流。
 *
 * 为什么与 CodeMirror 分开（编辑器实例在 `useCodeMirror.ts`）：这一份是**数据** ——
 * 手里那几个 ref、槽位里有没有草稿、这一版怎么存；那一份是**视图**，只管把文档
 * 显示出来、把改动与光标报上来。两边只通过两个口子碰面：外面改了内容时
 * `syncText` 推进编辑器，编辑器里改了内容时 `onDocChanged` 推进这里。
 *
 * 为什么自己管 `core/editor-state.ts`：光标与滚动的"原位还原"是**编辑状态的一部分**
 * （它记的也是正文），所以 `recordEditing` / `restoreEditing` 住在这里；那一模块只管
 * 按标签页 id 存一份，与这一篇的状态无关。
 *
 * 自动保存的计时器也归这里 —— 它节流的是"写草稿槽位"，写盘是这一份的活。
 * 离开页面时必须**先**把它清掉、再把内容落一次盘，顺序有讲究，都写在
 * `flushOnLeave()` 上（组件在 `onBeforeUnmount` 里调它，然后再拆编辑器）。
 */

import { invoke } from "@tauri-apps/api/core";
import { nextTick, ref, type Ref } from "vue";
import type { EditorView } from "@codemirror/view";
import { forget as forgetEditing, recall, remember } from "../core/editor-state.ts";
import { protection } from "../core/preferences.ts";
import { requestSyncAfterCommit } from "../core/sync.ts";
import { policyFrom, type Draft, type Note, type Policy, type Reading } from "../ipc/note.ts";

export interface NoteEditingHost {
    /** 正在编辑的标题 */
    title: () => string;
    /** 这个编辑器属于哪个标签页（给了才有"原位还原"这一层） */
    tabId: () => string | undefined;
    /** CodeMirror 实例：光标与滚动从这里读、往这里写；还没建起来时是 null */
    view: Ref<EditorView | null>;
    /** 提交完成后要去的地址：这篇笔记的阅读地址。地址归上层管，这里只把要去哪派发出去 */
    navigate: (title: string) => void;
}

export interface NoteEditing {
    /** 编辑器里的源码。由这里持有：装载、恢复草稿、自动保存都基于它 */
    markdown: Ref<string>;
    /** 上一次提交的内容：草稿与它一致就没什么可存的 */
    committedMarkdown: Ref<string>;
    /** 笔记装载成功了没有 —— 没成功就不做保存 / 提交这类写操作 */
    hasNote: Ref<boolean>;
    /** 槽位里有没有草稿 —— 草稿是每篇一个可覆盖槽位，只有"有 / 没有" */
    hasDraft: Ref<boolean>;
    /** 正在装载或提交时禁掉按钮，避免连点 */
    busy: Ref<boolean>;
    /** 装载中 */
    loading: Ref<boolean>;
    /** 装载失败的原因 */
    loadProblem: Ref<string>;
    /** 这一篇当前版读不出来：需要口令 */
    locked: Ref<boolean>;
    /** 上次输的那把是错的（与"还没输过"要分开说，人才知道该干什么） */
    wrongPassphrase: Ref<boolean>;
    /** 状态行：已恢复草稿 / 已保存 / 提交失败 */
    status: Ref<string>;
    /** 提交摘要，可留空 */
    summary: Ref<string>;
    /** 这一版怎么存（编辑器"存储："那一栏） */
    perCommit: Ref<Policy>;
    /** 这一篇的口令。只在这次提交要套对称层时用得上；交给后端会话后就不再留着 */
    passphraseDraft: Ref<string>;
    /** 槽位里那份还没决定要不要的草稿 */
    pendingDraft: Ref<Draft | null>;

    /** 装载：读笔记，再读它槽位里的草稿 */
    load: () => Promise<void>;
    /** 不看旧内容，直接写新的一版 */
    writeAnyway: () => void;
    /** 写草稿槽位 */
    saveDraft: () => Promise<void>;
    /** 提交 */
    commit: () => Promise<void>;
    /** 恢复槽位里那份草稿 */
    restoreDraft: () => void;
    /** 不要那份草稿：清掉槽位，编辑器留在已提交的这一版 */
    discardPending: () => Promise<void>;
    /** 放弃草稿：清掉槽位，回到上一次提交的内容 */
    discard: () => Promise<void>;
    /** 敲过键之后延迟自动保存 */
    scheduleAutosave: () => void;
    /** 记下这一刻的样子（字、光标、滚动）—— 切走时靠它还原 */
    recordEditing: () => void;
    /** 把光标与滚动放回原位 */
    restoreEditing: (state: { cursor: number; scroll: number }) => void;
    /**
     * 把欠下的那一次还原补上 —— 装载早于编辑器建起来时用。
     *
     * 由组件在 `codeMirror.mount()` **之后**调一次；没有欠账时它什么都不做。
     */
    applyPendingRestore: () => void;
    /** 离开这一页：清自动保存计时器、记下样子、把草稿落一次盘 */
    flushOnLeave: () => void;
}

/** 自动保存：停手三秒后把缓冲区写进草稿槽位 */
const AUTOSAVE_DELAY_MS = 3000;

export function useNoteEditing(host: NoteEditingHost): NoteEditing {
    const { title, tabId, view } = host;

    const markdown = ref("");
    const committedMarkdown = ref("");
    const hasNote = ref(false);
    const hasDraft = ref(false);
    const busy = ref(false);
    const loading = ref(false);
    const loadProblem = ref("");
    const locked = ref(false);
    const wrongPassphrase = ref(false);
    const status = ref("");
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

    /**
     * 槽位里那份**还没决定要不要**的草稿。
     *
     * 打开编辑器默认看到的是已提交的那一版 —— 草稿是"上次写了一半"，要不要接着写
     * 得人点头。在决定之前自动保存会让路，免得一敲键盘就把它盖掉。
     */
    const pendingDraft = ref<Draft | null>(null);

    let autosaveTimer: number | undefined;

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
                title: title(),
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
            const draft = await invoke<Draft | null>("load_draft", { title: title() });
            hasDraft.value = draft !== null;
            if (draft && draft.markdown !== markdown.value) {
                pendingDraft.value = draft;
            }
        } catch (error) {
            console.debug("读取草稿失败:", error);
        }

        // 这个标签页自己还留着编辑状态（切走又切回来）：直接用它。
        //
        // 它比草稿槽位**新** —— 草稿是攒一会儿才落一次盘的备份，而这份是切走那一刻
        // 编辑器里的原样。所以有它就不摆"要不要恢复草稿"那一条：那份是兜底，
        // 不是要问人的那一份。
        const remembered = recall(tabId());
        if (remembered) {
            markdown.value = remembered.markdown;
            pendingDraft.value = null;
            hasDraft.value = true;
            await nextTick();
            restoreEditing(remembered);
        }

        loading.value = false;
    }

    /**
     * 装载已经要还原、但编辑器还没建起来时的那一份。
     *
     * 为什么需要它：`load()` 一定跑在 `mount()` **之前** —— CodeMirror 的初始文档
     * 就是装载的结果，所以顺序反不了。而 `load()` 里正好要调 `restoreEditing`，
     * 那一刻 `view` 还是 `null`。原来那份实现在这里直接 return，于是
     * "切回这个标签页时视线落回原处"只在"重试"那条路上生效（那时编辑器已经在了）。
     *
     * 所以先把这份记下来，等编辑器建好之后由组件补一次
     * [`applyPendingRestore`]。
     */
    let pendingRestore: { cursor: number; scroll: number } | null = null;

    /** 真的把光标与滚动放回原位（前提是编辑器已经在了） */
    function applyRestore(editor: EditorView, state: { cursor: number; scroll: number }) {
        // 位置可能被 clamp：正文在这一趟里变短过，旧的 offset 未必还落在文档里
        const at = Math.max(0, Math.min(state.cursor, editor.state.doc.length));
        editor.dispatch({ selection: { anchor: at } });
        editor.scrollDOM.scrollTop = state.scroll;
        editor.focus();
    }

    /** 把光标与滚动放回原位（文档内容由外面那个 `watch(markdown)` 推进编辑器） */
    function restoreEditing(state: { cursor: number; scroll: number }) {
        const editor = view.value;
        if (!editor) {
            pendingRestore = state;
            return;
        }
        applyRestore(editor, state);
    }

    /**
     * 编辑器刚建起来时，把上面欠下的那一次补上。
     *
     * 没欠账、或者编辑器仍然不在（这一页压根没建编辑器）时什么都不做 —— 欠账要
     * **留着**，不能因为这次没补上就丢掉。
     */
    function applyPendingRestore() {
        if (!pendingRestore) {
            return;
        }
        const editor = view.value;
        if (!editor) {
            return;
        }
        const state = pendingRestore;
        pendingRestore = null;
        applyRestore(editor, state);
    }

    /** 记下这一刻的样子（字、光标、滚动）—— 切走时靠它还原 */
    function recordEditing() {
        const editor = view.value;
        const id = tabId();
        if (!id || !editor) {
            return;
        }
        remember(id, {
            markdown: markdown.value,
            cursor: editor.state.selection.main.head,
            scroll: editor.scrollDOM.scrollTop,
        });
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
            await invoke("save_draft", { title: title(), markdown: markdown.value });
            hasDraft.value = true;
            status.value = `已自动保存为草稿（${new Date().toLocaleTimeString()}）`;
        } catch (error) {
            status.value = `草稿保存失败：${String(error)}`;
        }
    }

    /** 输入之后延迟自动保存，连续敲字不会每次都写 */
    function scheduleAutosave() {
        window.clearTimeout(autosaveTimer);
        autosaveTimer = window.setTimeout(() => void saveDraft(), AUTOSAVE_DELAY_MS);
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
                title: title(),
                markdown: markdown.value,
                summary: summary.value.trim() || null,
                protection: perCommit.value,
                // 口令跟着这次写入记给新版本；它只活在这次会话里
                passphrase: perCommit.value.symmetric ? passphraseDraft.value || null : null,
            });
            passphraseDraft.value = "";
            status.value = "";
            // 这一趟编辑结束了：记忆清掉（再进来该看已提交的那一版）
            const id = tabId();
            if (id) {
                forgetEditing(id);
            }
            // 提交之后顺手叫一次同步（攒一会儿再跑，连提几次只同步一次）
            requestSyncAfterCommit();
            host.navigate(committed.title);
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
            await invoke<boolean>("discard_draft", { title: title() });
            hasDraft.value = false;
            const id = tabId();
            if (id) {
                forgetEditing(id);
            }
            markdown.value = committedMarkdown.value;
            status.value = "草稿已丢弃，已恢复为上次提交的内容";
        } catch (error) {
            status.value = `丢弃失败：${String(error)}`;
        } finally {
            busy.value = false;
        }
    }

    /**
     * 走之前把两件事做完。
     *
     * 1. 记下这一刻的样子 —— 切回来时光标与滚动还在原地；
     * 2. **把草稿落一次盘**。原来是直接取消自动保存的计时器，于是"敲完字马上切走"
     *    那一截就丢了（草稿要等计时器到点才写）。切标签页就是这么丢字的。
     *
     * 计时器必须**先**清掉：否则它到点时又会写一次，而那时组件已经在卸载了。
     */
    function flushOnLeave() {
        window.clearTimeout(autosaveTimer);
        autosaveTimer = undefined;
        recordEditing();
        void saveDraft();
    }

    return {
        markdown,
        committedMarkdown,
        hasNote,
        hasDraft,
        busy,
        loading,
        loadProblem,
        locked,
        wrongPassphrase,
        status,
        summary,
        perCommit,
        passphraseDraft,
        pendingDraft,
        load,
        writeAnyway,
        saveDraft,
        commit,
        restoreDraft,
        discardPending,
        discard,
        scheduleAutosave,
        recordEditing,
        restoreEditing,
        applyPendingRestore,
        flushOnLeave,
    };
}