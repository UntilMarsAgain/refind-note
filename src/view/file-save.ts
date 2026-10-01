/**
 * 另存为：把仓库里的文件存到用户选的位置。
 *
 * 走系统保存对话框（dialog 插件），再由后端把字节复制过去 —— 前端不碰文件内容。
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
