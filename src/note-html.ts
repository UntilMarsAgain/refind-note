//! 笔记 HTML 注入 DOM 之后的收尾工作。
//!
//! 三件事：把**相对地址**的图片与链接指向仓库里的文件、给图片挂上"点击看大图"、
//! 以及图片取不到时换一句说明。
//!
//! 放在这里而不是各个组件里，是因为阅读视图与编辑器预览都会注入同一份 HTML，
//! 行为该由同一处决定。
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { type MenuItem, openMenu } from "./context-menu";
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

/**
 * 笔记正文里的右键。
 *
 * 挂在根上做**委托**，而不是给每个元素各挂一份：正文会反复重渲染，
 * 逐元素挂处理器很容易漏掉新来的节点。
 *
 * 给什么项按点到的位置决定：图片（看大图、复制地址）、链接（复制地址）、
 * 选中的文字（复制）。都没有就**什么都不给** —— 交给全局那份去处理输入框。
 */
function attachContextMenu(root: HTMLElement) {
  if (root.dataset.menuReady) {
    return;
  }
  root.dataset.menuReady = "yes";

  root.addEventListener("contextmenu", (event) => {
    const target = event.target;
    const items: MenuItem[] = [];

    if (target instanceof HTMLImageElement) {
      items.push({ label: "看大图", run: () => viewImage(target.src, target.alt) });
      items.push({ label: "复制图片地址", run: () => writeText(target.src) });
    } else if (target instanceof HTMLAnchorElement) {
      // 内部链接上写的是笔记地址（`data-title`），外部链接就是 href
      const address = target.dataset.title || target.getAttribute("href") || "";
      items.push({ label: "复制链接地址", run: () => writeText(address) });
    }

    const selected = window.getSelection()?.toString().trim() ?? "";
    if (selected) {
      items.unshift({ label: "复制选中的文字", run: () => writeText(selected) });
    }

    if (items.length === 0) {
      return;
    }
    // 自己处理掉了：拦下默认，并且**不要**冒泡到全局那份（否则菜单刚开就被关掉）
    event.preventDefault();
    event.stopPropagation();
    openMenu(event, items);
  });
}

/** 笔记 HTML 注入之后的收尾 */
export function decorateNoteHtml(root: HTMLElement): void {
  attachContextMenu(root);
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
