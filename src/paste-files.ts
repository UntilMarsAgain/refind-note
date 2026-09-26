//! 剪贴板里的文件：挑出来、起个名字。
//!
//! 纯函数、无依赖，可直接测 —— "截图没有文件名，该叫什么"这种事定错了很难发现
//! （存进去一堆 `image.png`），而定对了就是一句话的事。

/** 浏览器给粘贴图片的默认名字，等于没名字 */
const PLACEHOLDERS = new Set(["image.png", "image.jpg", "image.jpeg", "image", "blob"]);

/** 常见类型 → 扩展名（只列真正会从剪贴板来的那几种） */
const EXTENSIONS: Record<string, string> = {
  "image/png": "png",
  "image/jpeg": "jpg",
  "image/gif": "gif",
  "image/webp": "webp",
  "image/svg+xml": "svg",
};

/**
 * 这一批剪贴板文件该叫什么名字（按顺序一一对应）。
 *
 * 截图/复制的图片往往没有真名字（浏览器给个 `image.png`），这时按类型起一个
 * `粘贴-YYYYMMDD-HHMMSS.png`：看得出是粘贴来的，也不会互相撞名（重名时后端还会加后缀）。
 */
export function pastedNames(files: { name: string; type: string }[], now: Date): string[] {
  const stamp = [
    now.getFullYear(),
    String(now.getMonth() + 1).padStart(2, "0"),
    String(now.getDate()).padStart(2, "0"),
    "-",
    String(now.getHours()).padStart(2, "0"),
    String(now.getMinutes()).padStart(2, "0"),
    String(now.getSeconds()).padStart(2, "0"),
  ].join("");

  return files.map((file, index) => {
    const name = file.name.trim();
    const useful = name !== "" && !PLACEHOLDERS.has(name.toLowerCase());
    if (useful) {
      return name;
    }
    const extension = EXTENSIONS[file.type] ?? "bin";
    const suffix = files.length > 1 ? `-${index + 1}` : "";
    return `粘贴-${stamp}${suffix}.${extension}`;
  });
}

/** 从剪贴板事件里取出的"文件"（截图、复制的图片、拷贝的文件都在这条路上） */
export function clipboardFiles(event: ClipboardEvent): File[] {
  const items = event.clipboardData?.items;
  if (!items) {
    return [];
  }
  const files: File[] = [];
  for (const item of Array.from(items)) {
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
