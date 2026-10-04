//   Refind Note is a note-taking software.
//   Copyright (C) 2026 Until Mars Again
//
//   This program is free software: you can redistribute it and/or modify
//   it under the terms of the GNU Affero General Public License as published by
//   the Free Software Foundation, either version 3 of the License, or
//   (at your option) any later version.
//
//   This program is distributed in the hope that it will be useful,
//   but WITHOUT ANY WARRANTY; without even the implied warranty of
//   MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
//   GNU Affero General Public License for more details.
//
//   You should have received a copy of the GNU Affero General Public License
//   along with this program.  If not, see <http://www.gnu.org/licenses/>.

/**
 * 笔记 HTML 注入 DOM 之后的收尾工作。
 *
 * 几件事：把相对地址换成能取到字节的附件地址、给正文挂上右键菜单、
 * 图片取不到时换一句说明、把音视频换成播放器、选项卡接线，
 * 以及让 `::js` 真的跑起来。
 *
 * 放在这里而不是各个组件里，是因为阅读视图与编辑器预览都会注入同一份 HTML，
 * 行为该由同一处决定。
 */

import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { type MenuItem, openMenu } from "./context-menu.ts";
import { scheduleDiagrams } from "./diagrams.ts";
import { decryptBox, wireDecrypt } from "./decrypt.ts";
import { fileTargetOf, fileUrl, vaultKeyOf } from "./file-links.ts";
import { saveNameOf, saveVaultFile, savableTitle } from "./file-save.ts";
import { fileInfo, freshUrl, readable } from "./file-unlock.ts";
import { viewImage } from "./image-viewer.ts";
import { renderMath } from "./math.ts";
import { wireTabs } from "./tabs.ts";

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
    // 本仓库的文件还在**问后端**"这一份是怎么回事"（加密？不存在？可读？）：
    // 这段时间里取不到字节是正常的，不能就此判它"不存在" ——
    // 加密的那些本来就取不到，而它们该看到的是解锁框（见 `attachVaultFile`）
    if (image.dataset.missing || image.dataset.vault === "pending") {
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
            const savable = savableTitle(target.src);
            items.push({ label: "看大图", run: () => viewImage(target.src, target.alt) });
            items.push({ label: "复制图片地址", run: () => writeText(target.src) });
            if (savable) {
                items.push({
                    label: "另存为…",
                    run: () =>
                        void saveVaultFile(savable, saveNameOf(target.src)).catch((error) =>
                            console.warn(error),
                        ),
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
            const savable = savableTitle(address);
            if (savable) {
                items.push({
                    label: "另存为…",
                    run: () =>
                        void saveVaultFile(savable, saveNameOf(address)).catch((error) =>
                            console.warn(error),
                        ),
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
 * 协议相对的地址（`//player.bilibili.com/…`）补齐成 `https://`。
 *
 * 这种写法的意思是"跟当前页面用同一个协议"，在普通网页里没问题；可**这一页的协议
 * 是应用自己的**（`tauri://`），照这条规则会去访问 `tauri://player.bilibili.com/…`，
 * 于是嵌进来的播放器、图床图片、外部脚本一律取不到东西。
 *
 * 网站上"分享 → 嵌入代码"给的基本都是这种写法（B 站、YouTube 都是），
 * 所以粘进来的东西能不能用，全看这一步补没补。
 */
function resolveProtocolRelative(root: HTMLElement) {
    const attributes = ["src", "href", "poster"];
    const selector = attributes.map((name) => `[${name}^="//"]`).join(", ");
    for (const element of root.querySelectorAll<HTMLElement>(selector)) {
        for (const name of attributes) {
            const value = element.getAttribute(name);
            if (value?.startsWith("//")) {
                element.setAttribute(name, `https:${value}`);
            }
        }
    }
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

/**
 * 让正文里的 `<script>` 真的跑起来（`::js`、以及 `::html js`）。
 *
 * `v-html` 走的是 `innerHTML`，而**这样插进去的脚本不会执行**（HTML 的规矩：
 * 只有解析器插入的脚本才跑）。于是 `::js` 只会是一段死字。这里的做法是把每个脚本
 * **重新装一次**：新建一个 `script` 节点、搬过属性与正文，替进文档 —— 这样就会执行。
 *
 * 新节点带 `data-ran`：同一棵树被收尾两次时不会重复执行。
 */
function runScripts(root: HTMLElement) {
    for (const stale of root.querySelectorAll("script")) {
        if (stale.dataset.ran) {
            continue;
        }
        const fresh = document.createElement("script");
        for (const attribute of stale.attributes) {
            fresh.setAttribute(attribute.name, attribute.value);
        }
        fresh.dataset.ran = "yes";
        fresh.text = stale.textContent ?? "";
        stale.replaceWith(fresh);
    }
}

/** 笔记 HTML 注入之后的收尾 */
export function decorateNoteHtml(root: HTMLElement): void {
    attachContextMenu(root);
    resolveProtocolRelative(root);
    resolveFileTargets(root);
    for (const image of root.querySelectorAll<HTMLImageElement>("img")) {
        // 反复注入（预览重渲染、开关来回切）时不要重复处理
        if (image.dataset.imageReady) {
            continue;
        }
        image.dataset.imageReady = "yes";

        // 本仓库里的文件：可能**加密存的**（那就不该直接去拉），
        // 也可能压根不是图（`![](片子.mp4)` —— markdown 一律渲染成 <img>，这里换成播放器）。
        // 先挂上"待判"的牌子：问明白之前，取不到字节不算"图片不存在"
        //
        // 名字要从**已经换过的**地址里反查（上面 `resolveFileTargets` 刚把它换成
        // `refind://…`，那个形式带协议，`fileTargetOf` 是不认的）
        const name = vaultKeyOf((image.getAttribute("src") ?? "").split("?")[0]);
        if (name) {
            image.dataset.vault = "pending";
        }
        attachImage(image);

        if (name) {
            void attachVaultFile(image, name);
        }
    }
    wireTabs(root);
    // 后端摆下的 `::decrypt` 标记（模板页上锁）—— 与附件那个框同一个构造器
    wireDecrypt(root);
    // 公式与图：都是"拿到元素再加工"，与上面几步同一类事
    renderMath(root);
    scheduleDiagrams(root);
    // 脚本放最后：跑起来时，正文该接的线都接好了
    runScripts(root);
}

/**
 * 加密的图片：**先摆一个"解锁"的地方，别让 `<img>` 去撞 404**。
 *
 * 文件进了 blob 仓就可能带口令层或 gpg 加密层 —— 那两种情况下"直接显示"是不成立的：
 * 后端取不到字节，图片只会加载失败。所以这里先问一句"这一版怎么存的、现在读不读得动"
 * （后端只看明文头，不需要口令），读不动就摆解锁框：口令层的当场输口令（原位，不跳页），
 * gpg 层的点一下就去读（由系统代理去问口令）。
 *
 * 框本身是 [`decryptBox`] 造的 —— 与正文里 `::decrypt` 那个框**同一个**。这里只负责
 * "探明之后把它摆出来"和"解锁之后把真正的图片换回去"。
 */
async function attachVaultFile(image: HTMLImageElement, name: string) {
    const info = await fileInfo(name);
    // 问明白了：往后取不到就是真取不到（图坏了、被删了），照常提示"不存在"
    delete image.dataset.vault;

    if (!info) {
        // 仓库里没有这一份 —— 照实说
        markMissing(image);
        return;
    }

    // 视频 / 音频：markdown 把它们写成了 `<img>`（`![]()` 只产出图片标签），
    // 换成真播放器。与 `::video` / `::audio` 摆出来的是同一副样子（同一套样式）
    const player = mediaTag(info.mime);
    if (player) {
        const element = document.createElement(player);
        element.className = "note-media";
        element.setAttribute("src", image.getAttribute("src") ?? info.url);
        element.setAttribute("controls", "controls");
        element.setAttribute("preload", "metadata");
        if (image.alt) {
            element.setAttribute("title", image.alt);
        }
        image.replaceWith(element);
        return;
    }

    if (!info.needs_unlock || readable(info)) {
        return;
    }

    const box = decryptBox({
        kind: "file",
        title: info.title,
        label: info.name,
        needsPassphrase: info.needs_passphrase,
        // 解锁之后：把真正的图片换回来。
        //
        // 注意这里换的是**框自己**，不是页面上的某个占位 —— 框是我们刚插进去的，
        // 按住它就不会与别人抢位置（正文明明已经有另一张图也是这个文件）。
        onRevealed: () => {
            const live = image.cloneNode() as HTMLImageElement;
            // 带个后缀，免得 webview 吃上一次的失败缓存（**上一次真的失败过**）
            live.src = freshUrl(image.getAttribute("src") ?? info.url);
            live.dataset.imageReady = "yes";
            attachImage(live);
            box.replaceWith(live);
        },
    });
    image.replaceWith(box);
}

/** 这个类型该用哪个播放器；不是音视频就是 null（那就照常当图片） */
function mediaTag(mime: string): "video" | "audio" | null {
    if (mime.startsWith("video/")) {
        return "video";
    }
    if (mime.startsWith("audio/")) {
        return "audio";
    }
    return null;
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
