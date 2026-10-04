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
import { printNoteAsPdf } from "./print.ts";

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
 * 导出成哪种格式。
 *
 * 与后端 `ExportFormat` 一一对应（那边是权威：认不认得这个词由它说了算）。
 */
export type ExportFormat = "markdown" | "html" | "pdf";

/** 各格式在对话框标题与提示里显示的名字 */
export const EXPORT_FORMAT_LABELS: Record<ExportFormat, string> = {
    markdown: "Markdown",
    html: "HTML",
    pdf: "PDF",
};

/** 各格式的扩展名（拼默认文件名用） */
export const EXPORT_FORMAT_EXTENSIONS: Record<ExportFormat, string> = {
    markdown: ".md",
    html: ".html",
    pdf: ".pdf",
};

/**
 * 导出某一版笔记；用户取消返回 `null`，PDF 返回 `null`（路径由浏览器决定）。
 *
 * `reference` 给版本 token 就导出那一版（地址里的 `@view-3` 那个 3）。
 *
 * **PDF 在这里返回 `null` 而不是路径**：那条路没有"文件落盘"这一步 ——
 * 它打开浏览器的打印面板，由用户在那里选「另存为 PDF」（理由见 `dom/print.ts`）。
 * 所以调用方不该拿它的返回值当"已保存的位置"去显示。
 */
export async function saveNoteAs(
    title: string,
    format: ExportFormat = "markdown",
    reference: string | null = null,
): Promise<string | null> {
    // PDF 走浏览器打印，没有"写到某个路径"这一步，先走它自己那条
    if (format === "pdf") {
        await printNoteAsPdf(title, reference);
        return null;
    }

    const extension = EXPORT_FORMAT_EXTENSIONS[format];
    const label = EXPORT_FORMAT_LABELS[format];

    if (isMobile()) {
        // 手机上没有保存对话框：后端统一放进下载目录，回来告诉我们落在哪
        return await invoke<string>("export_note", { title, reference, target: null, format });
    }
    const target = await save({
        defaultPath: `${noteFileName(title, extension)}`,
        title: `导出 ${label}`,
    });
    if (!target) {
        return null;
    }
    return await invoke<string>("export_note", { title, reference, target, format });
}

/**
 * 导出笔记的 markdown 原文；取消返回 null，存好了返回目标路径。
 *
 * 保留这个函数名是有理由的：调用处（页头那个"导出"）本来就说的是"导出"，
 * 而 markdown 是**默认**那一档 —— 改成别的格式是加了一个选择，不是替换了行为。
 */
export async function saveNoteMarkdown(
    title: string,
    reference: string | null = null,
): Promise<string | null> {
    return await saveNoteAs(title, "markdown", reference);
}

/**
 * 导出时的默认文件名：标题里的斜杠（子页面）与文件系统不认的字符都换成 `-`。
 *
 * `extension` 由格式定（`.md` / `.html` / `.pdf`）—— 与后端
 * `ExportFormat::extension` 同一份理由：拿 markdown 的名字去存 html，
 * 用户双击打开时浏览器会拿不准怎么渲染。
 */
export function noteFileName(title: string, extension = ".md"): string {
    const safe = title.replace(/[\\/:*?"<>|]/g, "-").trim();
    return `${safe || "笔记"}${extension}`;
}
