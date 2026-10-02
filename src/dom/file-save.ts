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
 * 另存为 / 导出：把仓库里的东西存到用户选的位置。
 *
 * 桌面上两条路都走系统保存对话框（dialog 插件），再由后端把内容复制过去 ——
 * 前端不碰文件内容，也不自己拼落盘的字节。
 * **手机上不问位置**（那边没有"选个文件夹"这回事）：后端统一放进下载目录，
 * 把落在哪告诉我们，我们再原样说给用户听。
 *
 * - [`saveVaultFile`]：`File:` 命名空间里的一份文件（图片、附件）；
 * - [`saveNoteMarkdown`]：一篇笔记的 **markdown 原文**（连模板记号一起带走）。
 */

import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import { isMobile } from "../core/platform.ts";
import { fileNameOfUrl, vaultKeyOf } from "./file-links.ts";

/** 把一个**文件页面**（`File:桥.png`）另存为；取消返回 null，存好了返回目标路径 */
export async function saveVaultFile(title: string, name?: string): Promise<string | null> {
    if (isMobile()) {
        // 手机上不问位置（没有那个对话框）：后端统一放进下载目录，回来告诉我们落在哪
        return await invoke<string>("export_file", { title, target: null });
    }
    const target = await save({ defaultPath: name ?? title, title: "另存为" });
    if (!target) {
        return null;
    }
    return await invoke<string>("export_file", { title, target });
}

/** 正文里一个地址对应的文件页面标题；不是本仓库的文件就是 null */
export function savableTitle(src: string): string | null {
    const name = vaultKeyOf(src);
    return name === null ? null : `File:${name}`;
}

/** 另存为对话框里的默认文件名 */
export function saveNameOf(src: string): string {
    return vaultKeyOf(src) ?? fileNameOfUrl(src);
}

/**
 * 导出笔记的 markdown 原文；取消返回 null，存好了返回目标路径。
 *
 * `reference` 给版本 token 就导出那一版（地址里的 `@view-3` 那个 3）。
 */
export async function saveNoteMarkdown(
    title: string,
    reference: string | null = null,
): Promise<string | null> {
    if (isMobile()) {
        // 同上：手机上进下载目录
        return await invoke<string>("export_note", { title, reference, target: null });
    }
    const target = await save({ defaultPath: noteFileName(title), title: "导出 Markdown" });
    if (!target) {
        return null;
    }
    return await invoke<string>("export_note", { title, reference, target });
}

/**
 * 导出时的默认文件名：标题里的斜杠（子页面）与文件系统不认的字符都换成 `-`，
 * 后缀是 `.md` —— 换台机器、换个编辑器，这个文件照样打得开。
 */
export function noteFileName(title: string): string {
    const safe = title.replace(/[\\/:*?"<>|]/g, "-").trim();
    return `${safe || "笔记"}.md`;
}
