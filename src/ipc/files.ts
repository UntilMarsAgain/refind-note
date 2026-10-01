/**
 * 文件 —— 与 Rust 侧 `src-tauri/src/features/files.rs` 一一对应。
 *
 * 文件就是 `File:` 命名空间里的**页面**：正文是字节，其余（版本、保护、回收站、
 * 整理）与笔记完全同一条路。
 */

import type { Protection } from "./note.ts";

export interface FileEntry {
    /** 显示标题（`File:桥.png`） */
    title: string;
    /** 页面名（`桥.png`）—— 笔记里引用的就是它 */
    name: string;
    mime: string;
    size: number;
    rev: number;
    modified: string;
    /** 取字节的地址（`refind://…`） */
    url: string;
    /** 这一版是怎么存的（明文头里就有，不解锁也报得出来） */
    protection: Protection;
    /** 读它要不要先解锁（口令层或 gpg 加密层） */
    needs_unlock: boolean;
}

/** 一个文件的现状：元信息 + 现在读不读得动 */
export interface FileInfo extends FileEntry {
    /** 口令正躺在本次会话里：为真时不用再问 */
    passphrase_ready: boolean;
    /** 这一版需要口令才能读 */
    needs_passphrase: boolean;
    /** 这一版是 gpg 加密的 */
    needs_secret_key: boolean;
}

/** 一次上传的结果 */
export interface Uploaded {
    entry: FileEntry;
    /** 这一版是更新（之前已经有这一页）还是新建 */
    updated: boolean;
}
