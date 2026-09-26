//! 笔记 HTML 注入 DOM 之后的收尾工作。
//!
//! 目前只有一件事：把**相对地址**的图片指向仓库里的文件。

/** 取件地址的前缀（与后端 `lib.rs` 里注册的 `refind:` 方案对应） */
const FILE_SCHEME = "refind://localhost/files/";

/**
 * 这一条 `src` 要不要换成取件地址？要就给出它。
 *
 * 纯函数、单独一步，因为**判断**才是容易出错的地方：哪些地址不该动，比哪些该动更难说清。
 *
 * 不动的三类：
//!
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
 * 把相对地址的图片指向仓库文件。
 *
 * 一条规则同时管住两种写法：markdown 的 `![名字](名字)` 与模板块的 `::image src=名字`。
 * 放在前端做，是因为笔记里写的是**文件名**，而取件地址由后端按名字或标识解析 ——
 * 两边都不必各自维护一份映射。
 */
export function resolveFileImages(root: HTMLElement): void {
  // `img` 与 `a` 用同一条规则：图片显示，其它附件点了交给系统打开
  for (const element of root.querySelectorAll<HTMLElement>("img, a[href]")) {
    // 反复注入（预览重渲染、开关来回切）时不要重复处理
    if (element.dataset.fileResolved) {
      continue;
    }
    const attribute = element.tagName === "A" ? "href" : "src";
    const target = fileTargetOf(element.getAttribute(attribute) ?? "");
    if (!target) {
      continue;
    }
    element.setAttribute(attribute, FILE_SCHEME + encodeURIComponent(target));
    element.dataset.fileResolved = "yes";
  }
}
