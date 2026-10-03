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
 * 文件列表页（`special:files`）那一列的状态与动作。
 *
 * 文件就是 `File:` 命名空间里的页面（`File:桥.png`）：传一次是新的一版，
 * **同一个名字再传就是更新**，旧版留在历史里；删除进回收站。所以这一页只管
 * 上传、更新、改名、另存为与删除 —— 版本与删除后的那些事，走的是与笔记同一条路。
 *
 * 为什么与 `FilesPage.vue` 分开：那一页剩下的是**摆出来的东西**（标题栏、说明段、
 * 那一列 `<li>` 与它的排版）；这里是**这一列背后发生的事**：读列表、收文件、
 * 两步确认的改名与删除、粘贴监听。两者变更的原因完全不同 —— 挪一下按钮的
 * 位置不该动到这里，而"删除要问两次"这种事也不该散在模板里。
 *
 * 与另一个文件页（`File:` 页面）的关系：共用的那几件动作在
 * `useFileActions.ts`，"这一版怎么存"那一栏在 `useFileStorage.ts`，
 * 那边自己管自己的一份。**删除**故意不共用：文件页的「删除」是跳到与笔记共用的
 * `@delete` 确认页上去（那一页对笔记与附件是同一套），只有这一列是在行内两步确认。
 */

import { onBeforeUnmount, onMounted, ref, type Ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { flash } from "../core/notice.ts";
import { requestSyncAfterCommit } from "../core/sync.ts";
import { clipboardFiles, uploadPasted } from "../dom/paste-files.ts";
import type { FileEntry, Uploaded } from "../ipc/files.ts";
import { useFileActions } from "./useFileActions.ts";
import { useFileStorage } from "./useFileStorage.ts";

/** 那一列背后的东西：状态 + 动作，模板只管把它们摆出来 */
export interface FileList {
  /** 读回来的这一列 */
  files: Ref<FileEntry[]>;
  loading: Ref<boolean>;
  busy: Ref<boolean>;
  /** 出错了说给用户听的那一句 */
  problem: Ref<string>;
  /** 正在改名的那个（标题）；空串是没有 */
  renaming: Ref<string>;
  /** 改名输入框里的草稿名 */
  renameText: Ref<string>;
  /** 正在"确认删除"的那个（标题）；两步确认 */
  confirming: Ref<string>;
  /** 正在"更新"的那个（标题）：更新要选一个文件，选之前先把名字记下来 */
  updating: Ref<string>;
  /** 「上传文件」：选一批，路径交给后端，字节不经过前端 */
  pick: () => Promise<void>;
  /** 给一个已有的文件传新版 */
  update: (file: FileEntry) => Promise<void>;
  /** 开始改名 / 确认改名 */
  startRename: (file: FileEntry) => void;
  saveRename: (file: FileEntry) => Promise<void>;
  /** 删除（第一遍只是"问一次"，第二遍才真删） */
  remove: (file: FileEntry) => Promise<void>;
  copyReference: (file: FileEntry) => Promise<void>;
  saveAs: (file: FileEntry) => Promise<void>;
  /** 这一栏"接下来传的东西"怎么存（与文件页那一份同源，各存各的） */
  storage: ReturnType<typeof useFileStorage>;
}

export function useFileList(): FileList {
  const files = ref<FileEntry[]>([]);
  const loading = ref(false);
  const busy = ref(false);
  const problem = ref("");

  const storage = useFileStorage();
  const actions = useFileActions({ busy, problem });

  /** 正在改名的那个（标题）与草稿名 */
  const renaming = ref("");
  const renameText = ref("");
  /** 正在"确认删除"的那个（标题）；两步确认 */
  const confirming = ref("");
  /** 正在"更新"的那个（标题）：更新要选一个文件，选之前先把名字记下来 */
  const updating = ref("");

  /** 读这一列 */
  async function load() {
    loading.value = true;
    problem.value = "";
    try {
      files.value = await invoke<FileEntry[]>("list_files");
    } catch (reason) {
      files.value = [];
      problem.value = String(reason);
    } finally {
      loading.value = false;
    }
  }

  onMounted(() => {
    void load();
    window.addEventListener("paste", onPaste);
  });

  onBeforeUnmount(() => window.removeEventListener("paste", onPaste));

  /** 这一页上按 Ctrl+V：把剪贴板里的文件收进来 */
  function onPaste(event: ClipboardEvent) {
    const picked = clipboardFiles(event);
    if (picked.length === 0) {
      return;
    }
    event.preventDefault();
    void collect(picked);
  }

  /** 收一批粘贴来的文件 */
  async function collect(picked: File[]) {
    busy.value = true;
    problem.value = "";
    try {
      const names: string[] = [];
      await uploadPasted(picked, async (bytes, name) => {
        const uploaded = await invoke<Uploaded>("upload_bytes", bytes, {
          // 走二进制通道时请求体整个是字节，参数只能放头里（后端从头上取）
          headers: storage.uploadHeaders(name),
        });
        names.push(uploaded.entry.name);
      });
      flash(`已上传：${names.join("、")}`);
      await load();
      // 上传/更新都是一版提交：顺手叫一次同步
      requestSyncAfterCommit();
    } catch (reason) {
      problem.value = String(reason);
    } finally {
      busy.value = false;
    }
  }

  /** 选文件上传：路径交给后端，字节不经过前端 */
  async function pick() {
    const picked = await open({ multiple: true, title: "选择要上传的文件" });
    if (!picked) {
      return;
    }
    const paths = Array.isArray(picked) ? picked : [picked];

    busy.value = true;
    problem.value = "";
    try {
      const names: string[] = [];
      for (const path of paths) {
        const uploaded = await invoke<Uploaded>("upload_file", {
          path,
          protection: storage.policy.value,
          passphrase: storage.passphraseFor(storage.policy.value),
        });
        names.push(uploaded.entry.name);
      }
      flash(`已上传：${names.join("、")}`);
      await load();
      // 上传/更新都是一版提交：顺手叫一次同步
      requestSyncAfterCommit();
    } catch (reason) {
      problem.value = String(reason);
    } finally {
      busy.value = false;
    }
  }

  /** 给一个已有的文件传新版：这就等于"更新"，旧版留在历史里 */
  async function update(file: FileEntry) {
    try {
      await actions.pushNewVersion(file, {
        policy: storage.policy.value,
        passphrase: storage.passphraseFor(storage.policy.value),
        onCommitting: () => {
          updating.value = file.title;
        },
        onCommitted: async (rev) => {
          flash(`已更新「${file.name}」到第 ${rev} 版（旧版仍在历史里）`);
          await load();
          // 上传/更新都是一版提交：顺手叫一次同步
          requestSyncAfterCommit();
        },
      });
    } finally {
      updating.value = "";
    }
  }

  function copyReference(file: FileEntry) {
    return actions.copyFileReference(file);
  }

  function saveAs(file: FileEntry) {
    return actions.saveFileAs(file);
  }

  /** 开始改名：把草稿名先填成现在的名字（人是在改，不是在重写） */
  function startRename(file: FileEntry) {
    renaming.value = file.title;
    renameText.value = file.name;
  }

  /**
   * 确认改名；没改或者是空的就当无事发生（省得问一次"确定吗"）。
   *
   * 改砸了**输入框留着**：那一行还停在改名形态里，人改了名字再按一次就行 ——
   * 把形态收掉的话，想重试就得先记住自己刚才打的是什么。
   */
  async function saveRename(file: FileEntry) {
    const name = renameText.value.trim();
    if (!name || name === file.name) {
      renaming.value = "";
      return;
    }

    const display = await actions.renameFile(file, name);
    if (display === null) {
      return;
    }
    renaming.value = "";
    await load();
  }

  /**
   * 删除：两遍。
   *
   * 第一遍只是把"确认删除"记在这一行上（第二遍才递命令）—— 删除进回收站、
   * 可以还原，但行内那串按钮里"删除"与"另存为"挨得太近，一按就删总归不像话。
   */
  async function remove(file: FileEntry) {
    if (confirming.value !== file.title) {
      confirming.value = file.title;
      return;
    }

    busy.value = true;
    try {
      await invoke("delete_file", { title: file.title });
      flash(`已删除「${file.name}」，可在回收站还原`);
      confirming.value = "";
      await load();
    } catch (reason) {
      problem.value = String(reason);
    } finally {
      busy.value = false;
    }
  }

  return {
    files,
    loading,
    busy,
    problem,
    renaming,
    renameText,
    confirming,
    updating,
    pick,
    update,
    startRename,
    saveRename,
    remove,
    copyReference,
    saveAs,
    storage,
  };
}