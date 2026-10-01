/**
 * 另存为 / 导出：把仓库里的东西存到用户选的位置。
 *
 * 两条路都走系统保存对话框（dialog 插件），再由后端把内容复制过去 ——
 * 前端不碰文件内容，也不自己拼落盘的字节。
 *
 * - [`saveVaultFile`]：`File:` 命名空间里的一份文件（图片、附件）；
 * - [`saveNoteMarkdown`]：一篇笔记的 **markdown 原文**（连模板记号一起带走）。
 */

import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import { fileNameOfUrl, vaultKeyOf } from "./file-links.ts";

/** 把一个**文件页面**（`File:桥.png`）另存为；取消返回 null，存好了返回目标路径 */
export async function saveVaultFile(title: string, name?: string): Promise<string | null> {
    const target = await save({ defaultPath: name ?? title, title: "另存为" });
    if (!target) {
        return null;
    }
    await invoke("export_file", { title, target });
    return target;
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
    const target = await save({ defaultPath: noteFileName(title), title: "导出 Markdown" });
    if (!target) {
        return null;
    }
    await invoke("export_note", { title, reference, target });
    return target;
}

/**
 * 导出时的默认文件名：标题里的斜杠（子页面）与文件系统不认的字符都换成 `-`，
 * 后缀是 `.md` —— 换台机器、换个编辑器，这个文件照样打得开。
 */
export function noteFileName(title: string): string {
    const safe = title.replace(/[\\/:*?"<>|]/g, "-").trim();
    return `${safe || "笔记"}.md`;
}
