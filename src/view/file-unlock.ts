/**
 * 加密文件的解锁。
 *
 * 文件进了 blob 仓，就与笔记一样可能带口令层或 gpg 加密层 —— 于是"把这张图显示出来"
 * 这件事不再是无条件的：**先问后端"这一版怎么存的、现在读不读得动"**，
 * 读不动就摆一个解锁按钮，而不是让 `<img>` 去撞一堵 404 的墙。
 */

import { invoke } from "@tauri-apps/api/core";
import type { FileInfo } from "../bindings/files.ts";

/** 问一份文件的现状（不读字节，只看明文头与本次会话里有没有口令） */
export async function fileInfo(name: string): Promise<FileInfo | null> {
    try {
        // 正文里引用的都是**最新一版**，所以 reference 明确给 null
        return await invoke<FileInfo>("file_info", { key: name, reference: null });
    } catch {
        // 不是文件、或者这一页不存在：调用方按"没有这回事"处理
        return null;
    }
}

/**
 * 解锁一个文件（把口令交给本次会话）。
 *
 * 解的是**显示标题**（`File:桥.png`）——口令按"页面 + 版本"存，与笔记同一套。
 */
export async function unlockFile(title: string, passphrase: string): Promise<void> {
    await invoke("unlock", { title, reference: null, passphrase });
}

/** 这一份现在读得动吗：不需要解锁，或者口令已经在手 */
export function readable(info: FileInfo): boolean {
    if (!info.needs_unlock) {
        return true;
    }
    // gpg 那一层由系统代理管：直接去读就是了，读的时候它自己会问
    if (info.needs_secret_key && !info.needs_passphrase) {
        return true;
    }
    return info.passphrase_ready;
}

/** 带一个"这次是新读的"后缀：解锁之后要让 webview 重新去取，而不是吃缓存 */
export function freshUrl(url: string): string {
    // 地址里可能已经带了查询（`?rev=3` —— 看的是历史里的某一版），那就用 `&` 接上
    const separator = url.includes("?") ? "&" : "?";
    return `${url}${separator}t=${Date.now()}`;
}
