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
 * 附件的两页**共用的那几件动作**：传新版、改名、复制引用、另存为。
 *
 * 为什么抽到一处：附件列表页（`special:files`）与单个文件页（`File:桥.png`）能把的
 * 就是这几件，而它们调的是**同一条命令**（`update_file` / `rename_file` /
 * `export_file`）—— 同一件事写两遍，改了一处忘了另一处是很容易的事（尤其
 * "传新版"那串参数：保护方式与口令必须与界面上的那一栏一致）。
 *
 * 这一层只管**把动作办完**：选完之后说给用户听的话、以及"办成了接着做什么"
 * （重列、重读、要不要顺手叫一次同步）是各页自己的事，由调用方在返回值上决定。
 * 失败一律写进宿主的 `problem`（界面上那一行红字），而 `busy` 由这里统一置位与复位。
 *
 * 它与两页的关系是"共用"而不是"属于"：两个页面各自调用一次，所以状态
 * （`busy` / `problem`）由调用方当参数递进来，而不是这里自己造一份。
 */

import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import type { Ref } from "vue";
import { flash } from "../core/notice.ts";
import { fileReferenceOf } from "../dom/file-links.ts";
import { saveVaultFile } from "../dom/file-save.ts";
import type { FileEntry, Uploaded } from "../ipc/files.ts";
import type { Policy } from "../ipc/note.ts";

/** 这一层要用的两样页面状态（由调用方造，理由见文件头） */
export interface FileActionsHost {
  /** 忙标记：动作期间禁掉按钮 */
  busy: Ref<boolean>;
  /** 出错了说给用户听的那一句 */
  problem: Ref<string>;
}

/** 传新版时"这一版怎么存"，以及"办完前后要说一声" */
export interface NewVersionRequest {
  /** 与笔记同一套封装策略 */
  policy: Policy;
  /** 口令层的口令；不套口令层就是 null */
  passphrase: string | null;
  /** 选完文件、真的要提交那一刻说一声（列表页据此把那一行标成"更新中…"） */
  onCommitting?: () => void;
  /**
   * 提交成功了说一声：重列一遍、要不要顺手叫同步，都在这句里。
   *
   * 它被**等着**办完才把 `busy` 放开 —— 与原先"按钮要一直灰到新的一列摆出来为止"一致。
   */
  onCommitted?: (rev: number) => void | Promise<void>;
}

export interface FileActions {
  /** 传新版：选文件 → 提交。返回新的一版的 rev；人取消了选文件或失败了就是 null */
  pushNewVersion: (file: FileEntry, request: NewVersionRequest) => Promise<number | null>;
  /** 改名：返回后端给的**规范标题**（页面名变了，显示标题跟着变）；失败是 null */
  renameFile: (file: FileEntry, name: string) => Promise<string | null>;
  /** 复制引用：把笔记里该写的那一句放进剪贴板 */
  copyFileReference: (file: { name: string; mime: string }) => Promise<void>;
  /** 另存为：把这一版存到用户选的位置 */
  saveFileAs: (file: FileEntry) => Promise<void>;
}

export function useFileActions(host: FileActionsHost): FileActions {
  /**
   * 给一个已有的文件传新版：这就等于"更新"，旧版留在历史里。
   *
   * 选文件这一步在前面：人还没挑好之前不算"更新中"，所以 `onCommitting` 是
   * 选了之后才叫的。返回新的一版的 rev —— 说给用户听的措辞各页不同（列表页要带上
   * 文件名，文件页那一行本来就在文件名旁边），由 `onCommitted` 决定。
   */
  async function pushNewVersion(file: FileEntry, request: NewVersionRequest): Promise<number | null> {
    const path = await open({ multiple: false, title: `选择「${file.name}」的新内容` });
    if (!path || Array.isArray(path)) {
      return null;
    }

    request.onCommitting?.();
    host.busy.value = true;
    try {
      const uploaded = await invoke<Uploaded>("update_file", {
        title: file.title,
        path,
        protection: request.policy,
        passphrase: request.passphrase,
      });
      await request.onCommitted?.(uploaded.entry.rev);
      return uploaded.entry.rev;
    } catch (reason) {
      host.problem.value = String(reason);
      return null;
    } finally {
      host.busy.value = false;
    }
  }

  /**
   * 改名：文件名就是页面名，所以改完要拿后端给的**规范标题**回去（显示标题跟着变）。
   *
   * 顺带把"笔记里写下的旧名称不会跟着改"说清楚 —— 笔记里写的是**名字**（那条规矩
   * 在 `dom/file-links.ts`），仓库改的是页面名，两者从此各说各的。
   */
  async function renameFile(file: FileEntry, name: string): Promise<string | null> {
    host.busy.value = true;
    try {
      const display = await invoke<string>("rename_file", { title: file.title, name });
      flash(`已改名为「${name}」；笔记里已写下的旧名称不会随之更改`);
      return display;
    } catch (reason) {
      host.problem.value = String(reason);
      return null;
    } finally {
      host.busy.value = false;
    }
  }

  /** 复制引用：放进剪贴板的是**笔记里该写的那一句**，不是 `refind://` 地址 */
  async function copyFileReference(file: { name: string; mime: string }) {
    const reference = fileReferenceOf(file);
    try {
      await writeText(reference);
      flash(`已复制引用：${reference}`);
    } catch (error) {
      flash(`复制失败：${error}`);
    }
  }

  /**
   * 另存为：把仓库里的这一版存到用户选的位置（对话框与落盘都在
   * `dom/file-save.ts` 那边，手机上不问位置）。取消就不说一句话。
   */
  async function saveFileAs(file: FileEntry) {
    try {
      const target = await saveVaultFile(file.title, file.name);
      if (target) {
        flash(`已另存为：${target}`);
      }
    } catch (error) {
      flash(`另存失败：${error}`);
    }
  }

  return { pushNewVersion, renameFile, copyFileReference, saveFileAs };
}