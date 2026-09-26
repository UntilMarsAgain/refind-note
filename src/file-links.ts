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

/**
 * 这是个可以下载的网址吗？是就原样给出（另存网页图片那条路只认 http/https）。
 */
export function httpUrlOf(source: string): string | null {
  const trimmed = source.trim();
  return trimmed.startsWith("http://") || trimmed.startsWith("https://") ? trimmed : null;
}

/**
 * 从一个网址猜另存时的默认文件名：取路径最后一段、去掉查询串与片段。
 *
 * 猜不出就给"图片" —— 这里图的是"默认名看着像话"，不值得为它做太多推理。
 * 坏编码不抛：文件名而已，原样用就行。
 */
export function fileNameOfUrl(url: string): string {
  let path = "";
  try {
    // 用 URL 解析而不是切字符串：这样"没有路径"（`https://example.com`）能认出来，
    // 否则会把主机名当成文件名（存成 `example.com` 很怪）
    path = new URL(url).pathname;
  } catch {
    // 不是合法网址：那就按字符串切，能猜多少算多少
    path = (url.split("?")[0] ?? "").split("#")[0] ?? "";
  }
  const last = path.split("/").filter((piece) => piece !== "").pop() ?? "";
  let name = last;
  try {
    name = decodeURIComponent(last);
  } catch {
    // 坏编码就用原样
  }
  return name === "" ? "图片" : name;
}

/** 笔记里引用这个附件时该写什么：图片写 markdown 图，其它写成链接 */
export function fileReferenceOf(file: { name: string; mime: string }): string {
  const name = file.name;
  return file.mime.startsWith("image/") ? `![${name}](${name})` : `[${name}](${name})`;
}
