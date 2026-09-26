//! 笔记 HTML 注入 DOM 之后的收尾工作。
//!
//! 三件事：把**相对地址**的图片与链接指向仓库里的文件、给图片挂上"点击看大图"、
//! 以及图片取不到时换一句说明。
//!
//! 放在这里而不是各个组件里，是因为阅读视图与编辑器预览都会注入同一份 HTML，
//! 行为该由同一处决定。
import { FILE_SCHEME, fileTargetOf } from "./file-links";
import { viewImage } from "./image-viewer";

/**
 * 图片取不到时**换成一句说明**，而不是留一个破图标：
 * 读者要知道的是"这张图不在了"，而不是盯着一个加载失败的方框猜。
 */
function markMissing(image: HTMLImageElement) {
  if (image.dataset.missing) {
    return;
  }
  image.dataset.missing = "yes";
  const placeholder = document.createElement("span");
  placeholder.className = "image-missing";
  placeholder.textContent = image.alt ? `图片不存在：${image.alt}` : "图片不存在";
  image.replaceWith(placeholder);
}

/** 笔记 HTML 注入之后的收尾 */
export function decorateNoteHtml(root: HTMLElement): void {
  // `img` 与 `a` 用同一条规则：图片显示，其它附件点了交给系统打开
  for (const element of root.querySelectorAll<HTMLElement>("img, a[href]")) {
    // 反复注入（预览重渲染、开关来回切）时不要重复处理
    if (element.dataset.fileResolved) {
      continue;
    }
    const attribute = element.tagName === "A" ? "href" : "src";
    const target = fileTargetOf(element.getAttribute(attribute) ?? "");
    if (target) {
      element.setAttribute(attribute, FILE_SCHEME + encodeURIComponent(target));
    }
    element.dataset.fileResolved = "yes";

    if (element instanceof HTMLImageElement) {
      attachImage(element);
    }
  }
}

/** 图片的两件事：点开看大图、取不到时给说明 */
function attachImage(image: HTMLImageElement) {
  image.classList.add("note-image");
  image.addEventListener("click", () => viewImage(image.src, image.alt));
  image.addEventListener("error", () => markMissing(image));
  // 已经失败过的（比如缓存里就是坏的）不会再触发 error，这里补一刀
  if (image.complete && image.naturalWidth === 0) {
    markMissing(image);
  }
}
