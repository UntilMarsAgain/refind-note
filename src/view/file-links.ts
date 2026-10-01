/**
 * 附件在正文里的写法与地址。
 *
 * 笔记里写的是**名字**（`![桥](桥.png)`）—— 那是人看得懂、也愿意改的东西；
 * 到了 webview 里才换成能取到字节的地址（`refind://…`）。两件事分开，
 * 于是"笔记里写什么"不依赖"东西存在哪"。
 */

import type { FileEntry } from "../bindings/files.ts";

/** 附件字节的地址前缀（Rust 侧注册的协议） */
export const FILE_SCHEME = "refind://localhost/files/";

/** 一段引用：图片用 `![]()`，其余用 `[]()` */
export function fileReferenceOf(file: { name: string; mime: string }): string {
    return file.mime.startsWith("image/")
        ? `![${file.name}](${file.name})`
        : `[${file.name}](${file.name})`;
}

/**
 * 正文里一个目标（`src` / `href`）该不该当成附件名。
 *
 * 不认的：空串、页内锚点（`#`）、站内绝对路径（`/`）、带协议的（`http:` / `data:` …）——
 * 那些各有各的意思，别抢。
 */
export function fileTargetOf(target: string): string | null {
    const trimmed = target.trim();
    if (!trimmed || trimmed.startsWith("#") || trimmed.startsWith("/") || trimmed.includes(":")) {
        return null;
    }
    return trimmed;
}

/** 附件名 → webview 能取的地址 */
export function fileUrl(name: string): string {
    return FILE_SCHEME + encodeURIComponent(name);
}

/** 反过来：地址 → 附件名（不是附件的地址就是 null） */
export function vaultKeyOf(src: string): string | null {
    if (!src.startsWith(FILE_SCHEME)) {
        return null;
    }
    try {
        return decodeURIComponent(src.slice(FILE_SCHEME.length));
    } catch {
        return null;
    }
}

/** 从一段地址里猜一个存盘用的名字（另存为的默认值） */
export function fileNameOfUrl(url: string): string {
    const path = url.split("?")[0]?.split("#")[0] ?? "";
    const tail = path.split("/").filter(Boolean).pop() ?? "";
    try {
        return decodeURIComponent(tail) || "附件";
    } catch {
        return tail || "附件";
    }
}

/** 这个附件是图片吗（决定缩略图与预览） */
export function isImage(file: FileEntry): boolean {
    return file.mime.startsWith("image/");
}
