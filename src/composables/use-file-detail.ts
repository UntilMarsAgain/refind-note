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
 * 单个文件页（`File:桥.png`）背后的东西：读哪一版、解锁、传新版、改名、交给系统打开。
 *
 * 文件页的正文是**字节**，不是给人读的文本 —— 所以这一页不渲染正文，而是把文件本身
 * 摆出来（能预览的预览，不能的给一句"下载看看"）。能做的事与笔记同源：
 * 看历史、删除（进回收站）、另存为；"改内容"这件事在这里叫**传新版**。
 *
 * 为什么与 `FileView.vue` 分开：那一页剩下的是**摆出来的东西**（页头、元信息、
 * 预览、动作条）；这里是**这一页背后发生的事**：读哪一版、能不能读得动、
 * 两步确认的"用系统应用打开"、页内改名。两者变更的原因不同 —— 挪一下预览的尺寸
 * 不该动到这里，而"加密的要先说清楚会写出明文"这种事不该散在模板里。
 *
 * 与文件列表页（`special:files`）的关系：共用的那几件动作在 `use-file-actions.ts`，
 * "这一版怎么存"那一栏在 `use-file-storage.ts`。**删除**故意不共用：这一页的
 * 「删除」是跳到与笔记共用的 `@delete` 确认页上去（那一页对笔记与附件是同一套）。
 */

import { computed, ref, watch, type Ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { Download, History, Pencil, Trash2, Upload } from "@lucide/vue";
import type { PageAction } from "../components/note/PageHeader.vue";
import { flash } from "../core/notice.ts";
import { readable as infoReadable } from "../dom/file-unlock.ts";
import type { FileEntry, FileInfo } from "../ipc/files.ts";
import { useFileActions } from "./use-file-actions.ts";
import { useFileStorage } from "./use-file-storage.ts";

/** 这一页由外面给的：看哪一页、往哪儿跳 */
export interface FileDetailSource {
  /** 显示标题（`File:桥.png`） */
  title: Ref<string>;
  /** 看哪一版（地址里的版本 token）；空 / null 就是最新一版 */
  reference: Ref<string | null | undefined>;
  /** 改完名要跳去后端给的规范标题；历史与删除也靠它跳 */
  navigate: (input: string) => void;
}

/** 这一页背后发生的事；模板只管把它们摆出来 */
export interface FileDetail {
  /** 这一份的现状（还没读出来就是 null） */
  entry: Ref<FileInfo | null>;
  /** 正在读第一遍（与"正忙"分开：重读那一版时按钮不该一起灰掉） */
  loading: Ref<boolean>;
  /** 正在办一件要等后端的事 */
  busy: Ref<boolean>;
  /** 出错了说给用户听的那一句 */
  problem: Ref<string>;
  /** 看的是不是一个旧版本 */
  older: Ref<boolean>;
  /** 这一份现在读得动吗（读不动就摆解锁框，不让 `<img>` 去撞一堵 404 的墙） */
  readable: Ref<boolean>;
  /** 原位输入的口令：只有口令层才用得上 */
  passphrase: Ref<string>;
  /** 正在改名：页内输入框摆出来没有 */
  renaming: Ref<boolean>;
  /** 改名输入框里的草稿名 */
  renameText: Ref<string>;
  /** "用系统应用打开"的明文提示已经摆出来了吗（两步确认，不用系统对话框） */
  confirmingOpen: Ref<boolean>;
  /** 页头那一排动作 */
  actions: PageAction[];
  /** 传新版时这一版怎么存（与上传页、编辑器同一套选择器） */
  storage: ReturnType<typeof useFileStorage>;
  /** 重读一遍（出错时那个"重试"按钮） */
  load: () => Promise<void>;
  /** 页头那排动作按下去之后各办各的 */
  onAction: (name: string) => void;
  /** 解锁：把口令交给本次会话，然后重读一遍 */
  unlock: () => Promise<void>;
  /** 传新版 */
  update: () => Promise<void>;
  saveAs: () => Promise<void>;
  copyReference: () => Promise<void>;
  startRename: () => void;
  saveRename: () => Promise<void>;
  cancelRename: () => void;
  /** 用系统应用打开（加密存的两步确认也在这里） */
  openWithSystem: () => Promise<void>;
}

/** 这一页背后发生的事 */
export function useFileDetail(source: FileDetailSource): FileDetail {
  const entry = ref<FileInfo | null>(null);
  const loading = ref(false);
  const busy = ref(false);
  const problem = ref("");

  /** 原位输入的口令：只有口令层才用得上 */
  const passphrase = ref("");

  /** 正在改名：页内输入框摆出来没有 */
  const renaming = ref(false);
  const renameText = ref("");

  /** "用系统应用打开"的明文提示已经摆出来了吗（两步确认，不用系统对话框） */
  const confirmingOpen = ref(false);

  const storage = useFileStorage();
  /** 共用的那几件动作跟着这一页自己的忙标记与出错那一行 */
  const shared = useFileActions({ busy, problem });

  /** 看的是不是一个旧版本 */
  const older = computed(() => {
    const found = entry.value;
    return source.reference.value != null && found != null && String(found.rev) !== source.reference.value;
  });

  /**
   * 读这一份文件的现状。
   *
   * 用的是 `file_info` 而不是 `list_files` —— 它顺带回答"这一版怎么存的、现在读不读得动"，
   * 于是加密的文件不会被当成普通图片直接去拉。
   */
  async function load() {
    loading.value = true;
    problem.value = "";
    try {
      entry.value = await invoke<FileInfo>("file_info", {
        key: source.title.value,
        reference: source.reference.value ?? null,
      });
    } catch (reason) {
      entry.value = null;
      problem.value = String(reason);
    } finally {
      loading.value = false;
    }
  }

  watch(() => [source.title.value, source.reference.value], () => void load(), { immediate: true });

  /**
   * 这一份现在读得动吗。
   *
   * 判法不在这里另写一份：正文里那张图（`dom/image-viewer.ts`）问的是同一件事，
   * 那条规矩写在 `dom/file-unlock.ts` 的 `readable()` 里 —— 两处各判一次，
   * 改了一边另一边就会安静地判错（表现为"解锁按钮不见了"）。
   */
  const readable = computed(() => (entry.value ? infoReadable(entry.value) : false));

  const actions: PageAction[] = [
    { name: "update", label: "传新版", icon: Upload },
    { name: "save", label: "另存为", icon: Download },
    { name: "history", label: "版本历史", icon: History },
    { name: "rename", label: "改名", icon: Pencil },
    { name: "delete", label: "删除", icon: Trash2, danger: true },
  ];

  function onAction(name: string) {
    const file = entry.value;
    if (!file) {
      return;
    }
    switch (name) {
      case "update":
        void update();
        break;
      case "save":
        void saveAs();
        break;
      case "history":
        source.navigate(`${file.title}@history`);
        break;
      case "rename":
        startRename();
        break;
      case "delete":
        source.navigate(`${file.title}@delete`);
        break;
    }
  }

  /** 解锁：把口令交给本次会话，然后重读一遍（这次就显示得出来了） */
  async function unlock() {
    const found = entry.value;
    if (!found) {
      return;
    }
    busy.value = true;
    try {
      await invoke("unlock", {
        title: found.title,
        // 看的是哪一版就解哪一版（口令按版本存）
        reference: source.reference.value ?? null,
        passphrase: passphrase.value,
      });
      passphrase.value = "";
      await load();
    } catch (reason) {
      problem.value = String(reason);
    } finally {
      busy.value = false;
    }
  }

  /** 传新版：选一个文件，内容替换进来，旧版留在历史里 */
  async function update() {
    const file = entry.value;
    if (!file) {
      return;
    }
    await shared.pushNewVersion(file, {
      policy: storage.policy.value,
      passphrase: storage.passphraseFor(storage.policy.value),
      onCommitted: async (rev) => {
        flash(`已更新到第 ${rev} 版（旧版仍在历史里）`);
        await load();
      },
    });
  }

  async function saveAs() {
    const file = entry.value;
    if (file) {
      await shared.saveFileAs(file);
    }
  }

  async function copyReference() {
    const file = entry.value;
    if (file) {
      await shared.copyFileReference(file);
    }
  }

  /**
   * 改名：文件名就是页面名，所以要问一句新名字。
   *
   * 就在这一页上问（一个输入框 + 确认/取消），**不借系统的文件对话框** ——
   * 系统给应用的那种窗口只有"打开/保存文件"，拿它当改名的输入框，
   * 看上去就像要把文件存到哪儿去，答非所问。
   */
  function startRename() {
    const file = entry.value;
    if (!file) {
      return;
    }
    // 输入框那一帧才摆出来，选中原名的事跟着它走（见 `FileRenameBar.vue`）
    renaming.value = true;
    renameText.value = file.name;
  }

  function cancelRename() {
    renaming.value = false;
  }

  /** 确认改名；没改或者是空的就当无事发生（不必再问一次"确定吗"） */
  async function saveRename() {
    const file = entry.value;
    if (!file) {
      return;
    }
    const name = renameText.value.trim();
    if (!name || name === file.name) {
      renaming.value = false;
      return;
    }

    const display = await shared.renameFile(file, name);
    if (display !== null) {
      renaming.value = false;
      // 页面名变了，显示标题跟着变：去后端给的那个**规范标题**
      source.navigate(display);
    }
  }

  /**
   * 交给系统的默认应用打开。
   *
   * 系统要的是**路径**，而仓库里存的是字节 —— 所以后端先把这一版落到临时目录里，
   * 再把那个路径交出去（拷贝用完由系统回收，不进仓库）。
   * 加密存的那几种，这一步等于在临时目录里留下一份**明文**：先把这件事说清楚再开。
   */
  async function openWithSystem() {
    const file = entry.value;
    if (!file) {
      return;
    }
    // 加密存的要先问一句：这一步会在临时目录里留下一份**明文**
    if (sealed(file) && !confirmingOpen.value) {
      confirmingOpen.value = true;
      return;
    }
    confirmingOpen.value = false;

    busy.value = true;
    try {
      const staged = await invoke<string>("open_file", {
        title: file.title,
        reference: source.reference.value ?? null,
      });
      flash(`已交给系统打开（临时副本：${staged}）`);
    } catch (reason) {
      problem.value = String(reason);
    } finally {
      busy.value = false;
    }
  }

  return {
    entry,
    loading,
    busy,
    problem,
    older,
    readable,
    passphrase,
    renaming,
    renameText,
    confirmingOpen,
    actions,
    storage,
    load,
    onAction,
    unlock,
    update,
    saveAs,
    copyReference,
    startRename,
    saveRename,
    cancelRename,
    openWithSystem,
  };
}

/** 这一份在仓库里是**加密存的**吗（口令层或 gpg 加密层） */
function sealed(file: FileEntry): boolean {
  return file.protection.symmetric || file.protection.encrypt !== null;
}