//! 笔记里的地址：哪一些指的是**仓库里的文件**。
//!
//! 单独一个模块、不引入任何依赖，因为这是"笔记 HTML 收尾"里最容易出错的一步判断
//! （哪些地址不该动，比哪些该动更难说清），而它完全不需要 DOM，可以直接测。
//! DOM 那一侧在 `note-html.ts`。

/** 取件地址的前缀（与后端 `lib.rs` 里注册的 `refind:` 方案对应） */
export const FILE_SCHEME = "refind://localhost/files/";

/**
 * 这一条 `src`/`href` 要不要换成取件地址？要就给出它。
 *
 * 不动的三类：
 *
 * - 绝对地址（`http:`、`https:`、`asset:`、`data:` —— 带冒号的一律当绝对地址）；
 * - 以 `/` 开头的（那是**程序自己**的静态资源，例如 `/logo.svg`）；
 * - 空串与纯片段（`#` 开头）。
 *
 * 其余（`图片.png`、`./图/桥.png`）都当成仓库里的文件 —— 笔记里写文件名，
 * 是目前唯一"人记得住"的写法。
 */
export function fileTargetOf(source: string): string | null {
  const trimmed = source.trim();
  if (trimmed === "" || trimmed.startsWith("#")) {
    return null;
  }
  if (trimmed.startsWith("/") || trimmed.includes(":")) {
    return null;
  }
  return trimmed;
}

/**
 * 这颗图片是不是**仓库里的文件**？是就给出它的键（显示名）。
 *
 * 反解我们自己写上去的那种地址。外部图片（http）不算 —— 它们的字节拿不到（跨域），
 * 所以"另存为"这一项不该出现在它们身上：给一个点了没用的项，比不给更糟。
 */
export function vaultKeyOf(source: string): string | null {
  if (!source.startsWith(FILE_SCHEME)) {
    return null;
  }
  try {
    const key = decodeURIComponent(source.slice(FILE_SCHEME.length));
    return key === "" ? null : key;
  } catch {
    // 坏编码不抛：这只是"认不认得出来"的判断，认不出就当不是
    return null;
  }
}

/** 笔记里引用这个附件时该写什么：图片写 markdown 图，其它写成链接 */
export function fileReferenceOf(file: { name: string; mime: string }): string {
  const name = file.name;
  return file.mime.startsWith("image/") ? `![${name}](${name})` : `[${name}](${name})`;
}
