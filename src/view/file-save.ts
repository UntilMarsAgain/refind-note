/**
 * 另存为：把仓库里的附件/图片存到用户选的位置。
 *
 * 走系统保存对话框（dialog 插件），再由后端把字节复制过去 —— 前端不碰文件内容。
 */

import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import { fileNameOfUrl, vaultKeyOf } from "./file-links.ts";

/** 把一个附件另存为；取消返回 null，存好了返回目标路径 */
export async function saveVaultFile(key: string): Promise<string | null> {
    const target = await save({ defaultPath: key, title: "另存为" });
    if (!target) {
        return null;
    }
    await invoke("export_file", { key, target });
    return target;
}

/** 正文里一个地址能不能另存（附件地址才行） */
export function savableKey(src: string): string | null {
    return vaultKeyOf(src);
}

/** 存盘时给个体面的默认名 */
export function defaultSaveName(src: string): string {
    return vaultKeyOf(src) ?? fileNameOfUrl(src);
}
