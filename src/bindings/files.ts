/**
 * 附件 —— 与 Rust 侧 `src-tauri/src/files.rs` 一一对应。
 */

export interface FileEntry {
    /** 标识：文件名与地址都用它 */
    id: string;
    /** 显示名（笔记里引用的就是它） */
    name: string;
    /** 后缀；只影响 mime，改名不动它 */
    extension: string;
    size: number;
    sha256: string;
    uploaded: string;
    mime: string;
}

/** 一次上传的结果 */
export interface Uploaded {
    entry: FileEntry;
    /** 内容与后缀都一样，直接复用了已有的那一份 */
    reused: boolean;
}
