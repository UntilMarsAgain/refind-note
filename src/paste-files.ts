/**
 * 粘贴板里的文件：从粘贴事件里把它们捞出来，并给个像样的名字。
 *
 * 剪贴板里的图片常常没有名字（叫 `image.png`、`blob` 之类），落进仓库就是一堆
 * 分不清谁是谁的文件。所以这里按时间给一个 `粘贴-20260101-120000.png`，
 * 一次粘好几张再加序号。
 */

/** 这些名字是剪贴板临时起的，不算真的名字 */
const PLACEHOLDER_NAMES = new Set(["image.png", "image.jpg", "image.jpeg", "image", "blob"]);

/** MIME → 后缀（`File.type` 只在这个表里才拿来当后缀用） */
const MIME_EXTENSIONS: Record<string, string> = {
    "image/png": "png",
    "image/jpeg": "jpg",
    "image/gif": "gif",
    "image/webp": "webp",
    "image/svg+xml": "svg",
};

/** 粘贴事件里带的文件（没有就是空数组） */
export function clipboardFiles(event: ClipboardEvent): File[] {
    const items = event.clipboardData?.items;
    if (!items) {
        return [];
    }

    const files: File[] = [];
    for (const item of items) {
        if (item.kind !== "file") {
            continue;
        }
        const file = item.getAsFile();
        if (file) {
            files.push(file);
        }
    }
    return files;
}

/**
 * 给这一批文件起名。名字已经像样的原样留着；剪贴板临时名换成时间戳。
 *
 * （文件系统里本来的合法名字不该被改掉 —— 人认得出自己放进去的那张图。）
 */
export function pastedNames(files: File[], now: Date): string[] {
    const stamp = [
        now.getFullYear(),
        pad(now.getMonth() + 1),
        pad(now.getDate()),
        "-",
        pad(now.getHours()),
        pad(now.getMinutes()),
        pad(now.getSeconds()),
    ].join("");

    const many = files.length > 1;
    return files.map((file, index) => {
        const raw = file.name.trim();
        const placeholder = !raw || PLACEHOLDER_NAMES.has(raw.toLowerCase());
        if (!placeholder) {
            return raw;
        }

        const extension = MIME_EXTENSIONS[file.type] ?? extensionOf(raw) ?? "bin";
        const suffix = many ? `-${index + 1}` : "";
        return `粘贴-${stamp}${suffix}.${extension}`;
    });
}

function pad(value: number): string {
    return String(value).padStart(2, "0");
}

function extensionOf(name: string): string | null {
    const dot = name.lastIndexOf(".");
    return dot > 0 ? name.slice(dot + 1) : null;
}

/** 把文件读成字节，交给后端落盘 */
export async function uploadPasted(
    files: File[],
    upload: (bytes: Uint8Array, name: string) => Promise<unknown>,
): Promise<number> {
    const names = pastedNames(files, new Date());
    let done = 0;

    for (const [index, file] of files.entries()) {
        const bytes = new Uint8Array(await file.arrayBuffer());
        await upload(bytes, names[index] ?? file.name);
        done += 1;
    }
    return done;
}
