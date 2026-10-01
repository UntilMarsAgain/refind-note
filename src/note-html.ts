/**
 * 笔记 HTML 注入 DOM 之后的收尾工作。
 *
 * 三件事：把相对地址换成能取到字节的附件地址、给正文挂上右键菜单、
 * 图片取不到时换一句说明。
 *
 * 放在这里而不是各个组件里，是因为阅读视图与编辑器预览都会注入同一份 HTML，
 * 行为该由同一处决定。
 */

import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { type MenuItem, openMenu } from "./context-menu.ts";
import { fileTargetOf, fileUrl, vaultKeyOf } from "./file-links.ts";
import { saveVaultFile, savableKey } from "./file-save.ts";
import { viewImage } from "./image-viewer.ts";

/**
 * "在新标签页打开"由 App 注入。
 *
 * 笔记正文是**后端渲染的 HTML**，这里拿不到 App 的事件通道，所以由 App 在启动时把实现
 * 塞进来 —— 与"点链接"走同一个函数，两处行为不会分家。
 */
let openInNewTab: ((title: string) => void) | null = null;

/** App 启动时调用一次 */
export function setOpenInNewTab(handler: (title: string) => void) {
    openInNewTab = handler;
}

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
 * 给什么项按点到的位置决定：图片（复制地址）、链接（在新标签页打开、复制地址）、
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
            const key = savableKey(target.src);
            items.push({ label: "看大图", run: () => viewImage(target.src, target.alt) });
            items.push({ label: "复制图片地址", run: () => writeText(target.src) });
            if (key) {
                items.push({
                    label: "另存为…",
                    run: () => void saveVaultFile(key).catch((error) => console.warn(error)),
                });
            }
        } else if (target instanceof HTMLAnchorElement) {
            // 内部链接上写的是笔记名（`data-title`），外部链接就是 href
            const title = target.dataset.title;
            const address = title || target.getAttribute("href") || "";
            if (title && openInNewTab) {
                // 与 Ctrl+点击同一件事：右键里也该有它，否则这条路只有键盘用户找得到
                items.push({ label: "在新标签页打开", run: () => openInNewTab?.(title) });
            }
            const key = vaultKeyOf(address);
            if (key) {
                items.push({
                    label: "另存为…",
                    run: () => void saveVaultFile(key).catch((error) => console.warn(error)),
                });
            }
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

/**
 * 相对地址 → 附件地址。
 *
 * 笔记里写的是名字（`![桥](桥.png)`），而 webview 取字节要有地址。认的只有
 * "既不是锚点、也不是绝对路径、也没有协议"的那一类（见 `fileTargetOf`）——
 * 其余的（外链、页内锚点）各有各的用法，不能抢。
 */
function resolveFileTargets(root: HTMLElement) {
    for (const element of root.querySelectorAll<HTMLElement>("img[src], a[href]")) {
        // 反复注入（预览重渲染）时不要重复处理
        if (element.dataset.fileResolved) {
            continue;
        }
        element.dataset.fileResolved = "yes";

        const attribute = element instanceof HTMLImageElement ? "src" : "href";
        const raw = element.getAttribute(attribute) ?? "";
        const target = fileTargetOf(raw);
        if (target) {
            element.setAttribute(attribute, fileUrl(decoded(target)));
        }
    }
}

/**
 * markdown 渲染器会把非 ASCII 的目标做一次百分号编码（`桥.png` → `%E6%A1%A5.png`），
 * 所以这里先把它解码回名字，再由 [`fileUrl`] 统一编码 —— 不然会编码两次，
 * 后端按名字就查不到了。
 */
function decoded(target: string): string {
    try {
        return decodeURIComponent(target);
    } catch {
        // 非法转义序列（名字里就有个 `%`）就按原样用
        return target;
    }
}

/** 笔记 HTML 注入之后的收尾 */
export function decorateNoteHtml(root: HTMLElement): void {
    attachContextMenu(root);
    resolveFileTargets(root);
    for (const image of root.querySelectorAll<HTMLImageElement>("img")) {
        // 反复注入（预览重渲染、开关来回切）时不要重复处理
        if (image.dataset.imageReady) {
            continue;
        }
        image.dataset.imageReady = "yes";
        attachImage(image);
    }
}

/** 图片：点一下看大图，加载不出来给一句说明 */
function attachImage(image: HTMLImageElement) {
    image.addEventListener("click", () => viewImage(image.src, image.alt));
    image.addEventListener("error", () => markMissing(image));
    // 已经失败过的（比如缓存里就是坏的）不会再触发 error，这里补一刀
    if (image.complete && image.naturalWidth === 0) {
        markMissing(image);
    }
}
