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
import { fileInfo, freshUrl } from "./file-unlock.ts";
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
 * 图片取不到时的两条出路：**解锁框**（这一份是加密的）或者**一句说明**。
 *
 * ## 为什么这里能分出这两者
 *
 * `<img>` 的 error 事件**看不见响应状态与头** —— 它只知道"没加载出来"，而
 * `platform::protocol` 把"没有这一份"、"上了锁"、"gpg 解密失败"一起答成 404。
 * 所以判据不能来自 error 事件，得来自**已经拿在手里的那份 `file_info`**
 * （`attachVaultFile` 把它挂在 `data-vault-locked` 上）：它说加密了就是解不开，
 * 说没加密就是这份东西真的坏了。
 *
 * ## 摆框而不是说"图片不存在"
 *
 * 这正是那个 bug：原来这一支直接说"图片不存在"、不留任何出路，于是人既不能重试
 * 也看不到按钮，而真相是**按一下就能让 gpg 去问**。
 *
 * 框是 [`decryptBox`] 造的，与正文里 `::decrypt` 那个框**同一个**。
 */
function markMissing(image: HTMLImageElement) {
    if (image.dataset.missing || image.dataset.vault === "pending") {
        return;
    }

    // 这一份是加密的 → 摆解锁框（`image` 自己就是替换掉的那个节点）
    if (image.dataset.vaultLocked === "yes") {
        // 先占位：`decryptBox` 的回调要用到这个框，而它是在框建好之后才被按的
        let box: HTMLElement | null = null;
        box = decryptBox({
            kind: "file",
            title: image.dataset.vaultTitle ?? "",
            label: image.dataset.vaultLabel || image.dataset.vaultTitle || "",
            needsPassphrase: image.dataset.vaultNeedsPassphrase === "yes",
            onRevealed: () => {
                if (!box) {
                    return;
                }
                const live = image.cloneNode() as HTMLImageElement;
                // 带个"这次是新读的"后缀，免得 webview 吃上一次失败的缓存
                live.src = freshUrl(image.getAttribute("src") ?? "");
                live.dataset.imageReady = "yes";
                // **把 `vault*` 那几个搬过去**：gpg 可能又失败一次（没有私钥、
                // 代理被取消），那时要再摆一个框，而不是这一次就改口说"图片不存在"。
                live.dataset.vaultLocked = image.dataset.vaultLocked ?? "";
                live.dataset.vaultTitle = image.dataset.vaultTitle ?? "";
                live.dataset.vaultLabel = image.dataset.vaultLabel ?? "";
                live.dataset.vaultNeedsPassphrase = image.dataset.vaultNeedsPassphrase ?? "no";
                attachImage(live);
                box.replaceWith(live);
            },
        });
        image.replaceWith(box);
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
 * 仓库里的文件（图片、音视频、附件）：先**照常显示**，加载不了再问为什么。
 *
 * ## 为什么不再"看见加密就摆框"
 *
 * 原来的做法是拿 `file_info` 的头去判断：`needs_unlock` 且"口令不在手"就摆解锁框。
 * 那个判断对 gpg 是**错的** —— gpg 大部分时候是自动的（钥匙的口令有缓存，或者
 * 智能卡碰一下就行），先摆一个框等于让人白点一下；而它对"自动解开了"的那些，
 * 本来什么都不用做。
 *
 * 现在的顺序按作者定的规格来：**先试**（让 `<img>` 去拉），**失败了再问**。
 *
 * ## 失败了怎么问
 *
 * `<img>` 的 error 事件看不见响应状态与头 —— 它只知道"没加载出来"。所以问的是
 * **已经拿在手里的那份 `file_info`**（不读字节，只看明文头）：
 *
 * - 它说没加密 → 那就不是锁的问题，是这份东西真的坏了（被删了、截断了）。
 *   说"图片不存在"，并**不**摆框 —— 输了口令问题还在，把人困在一个没用的输入框前。
 * - 它说加密了 → 那就是**解不开**。摆框，并且把 `file_info` 给不出的东西留给
 *   用户按按钮时去问后端（`resolve_decrypt` 真的去加载一次，见 `dom/decrypt.ts`）。
 */
async function attachVaultFile(image: HTMLImageElement, name: string) {
    const info = await fileInfo(name);
    // 问明了：往后 `<img>` 加载不到就是**真的**加载不到，可以照实说了
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

    if (!info.needs_unlock) {
        // 根本没加密：加载不到就是这份东西坏了/没了。`markMissing` 已经挂好了
        // （`attachImage` 里挂的），等它自己触发。
        return;
    }

    // 加密的：摆不摆框由**加载失败**决定，不由这里决定 —— 先把"这一份是加密的"
    // 这件事记在元素上，`markMissing` 认它
    image.dataset.vaultLocked = "yes";
    image.dataset.vaultTitle = info.title;
    image.dataset.vaultLabel = info.name;
    image.dataset.vaultNeedsPassphrase = info.needs_passphrase ? "yes" : "no";

    // **补一刀**：图片有可能在 `fileInfo` 回来**之前**就加载失败了。那一次
    // `markMissing` 撞上 `data-vault="pending"` 就提前返回了（问明白之前不能判它
    // "不存在"），而失败**只发生一次** —— 探明之后没人再叫它，于是那一次失败
    // 被静静吃掉：既没有解锁框，也没有"图片不存在"。
    //
    // 这是既有行为里就有的漏，与本次改动无关，但正好落在同一条路上：不补这一刀，
    // "先试，失败再摆框"就有一段时间窗是黑的。
    if (image.isConnected && image.complete && image.naturalWidth === 0) {
        markMissing(image);
    }
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
