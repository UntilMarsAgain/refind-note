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
 * 页内解锁框 —— 模板页与加密附件共用。
 *
 * ## 为什么框只有这一个构造器
 *
 * 页内读不出来的份有两条来路，而**信息到达的时间不一样**：模板页是后端渲染时就知道
 * （`::decrypt` 只出 `span[data-decrypt]` 标记，见 `markdown/syntax/template/dispatch.rs`），
 * 附件是前端异步探明之后才知道。所以标记与"探明结果"从两个方向汇到
 * [`decryptBox`] 这一个函数 —— 框的样子与行为只有这一处定义，两条路不会长成两个样子。
 *
 * 钉它的形状靠两处测试：Rust 侧
 * `the_unlock_box_is_only_a_marker_so_both_callers_share_one_builder`
 * （后端不许自己造输入框与按钮），前端侧 `tests/web/decrypt.test.ts`。
 *
 * ## 那个 bug 是怎么修的
 *
 * 原来附件那条路是**先换掉框、再看结果**：点"显示" → 把解锁框换成 `<img>` → `<img>`
 * 去拉 → gpg 解密失败 → `platform::protocol` 把"解密失败"与"没有这一份"一起答成 404 →
 * 前端说"图片不存在"。框已经没了，人既不能重试也看不到按钮。
 *
 * 现在**成功是由 [`lockState`] 说出来的**，不是由 `<img>` 加载成功推出来的：
 * 后端真的去读了一次，读不动就把原因给回来。所以失败时框留在原地、按钮还能按、
 * 原因写在旁边。gpg 那一层没有口令，除了真读没有别的办法知道成不成 —— 这也是
 * 为什么 `readable` 必须是实测的而不是推断的。
 */

import { type LockKind, lockState } from "../ipc/lock.ts";

/**
 * 框有哪几样 —— 由"要不要口令"**唯一**决定。
 *
 * 抽成纯函数是为了能测：这一段是 DOM 造型的源头，而 `node --test` 里没有 DOM。
 * 抽出来之后，"gpg 的框不该有输入框"、"口令的框必须有"这两条就能被机器钉住，
 * 而不是靠人读 CSS（见 `tests/web/decrypt.test.ts`）。
 */
export interface BoxShape {
    /** 要不要摆口令输入框 */
    showsInput: boolean;
    /** 那句"接下来要做什么" */
    hint: string;
    /** 按钮上写什么 */
    button: string;
}

/** 框该长什么样 */
export function boxShape(needsPassphrase: boolean): BoxShape {
    return {
        showsInput: needsPassphrase,
        // 两句话都说明**下一步**，不只是"这是加密的" ——
        // 只说状态的话人还是不知道自己能干什么。
        hint: needsPassphrase ? "输入口令后显示" : "解锁后显示",
        button: "显示",
    };
}

/** 摆一个框要知道的 */
export interface DecryptSpec {
    kind: LockKind;
    /** 显示标题（`unlock` 与 `read` 都按它走） */
    title: string;
    /** 给人看的名字 */
    label: string;
    /** 要不要给口令输入框。gpg 那一层为假 —— 它问的是钥匙串 */
    needsPassphrase: boolean;
    /** 已经知道的失败原因（探明之前渲染的场合才有） */
    reason?: string;
    /** 解锁成功了做什么 */
    onRevealed: () => void | Promise<void>;
}

/**
 * "解锁之后重读一遍这一篇"由 App 注入。
 *
 * 笔记正文是**后端渲染的 HTML**，模板内容在渲染期就烤进去了 —— 解锁之后没法只补
 * 那一块，只能整篇重读。与 `note-html.ts` 的 `setOpenInNewTab` 同一套路：dom 这层
 * 拿不到 App 的事件通道，所以由 App 在启动时把实现塞进来。
 */
let rereadNote: ((title: string) => void) | null = null;

/** App 启动时调用一次 */
export function setRereadNote(handler: (title: string) => void) {
    rereadNote = handler;
}

/**
 * 造一个解锁框，并把它按好了。
 *
 * 框里的三样（名字、说明、按钮）与那条"为什么"都在这里，所以两条来路不会有第二份。
 *
 * ## 失败时框留在原地
 *
 * 这是整个模块最要紧的一条：`reveal` 先问后端，**读到 `readable: true` 才通知外面**。
 * 原来反过来做（先换框再看结果），于是 gpg 解密失败变成"图片不存在"且无法重试 ——
 * 见本文件抬头那段。
 */
export function decryptBox(spec: DecryptSpec): HTMLElement {
    const box = document.createElement("span");
    box.className = "unlock-box";
    box.dataset.unlockBox = "yes";
    box.dataset.decryptKind = spec.kind;
    box.dataset.decryptTitle = spec.title;

    const label = document.createElement("span");
    label.className = "unlock-box__label";
    label.textContent = spec.label;
    box.append(label);

    // 形状只有一处决定（boxShape），造 DOM 的代码不再自己判断
    const shape = boxShape(spec.needsPassphrase);

    const hint = document.createElement("span");
    hint.className = "unlock-box__hint";
    hint.textContent = shape.hint;
    box.append(hint);

    const input = document.createElement("input");
    input.type = "password";
    input.className = "unlock-box__input";
    input.placeholder = "口令";
    if (shape.showsInput) {
        box.append(input);
    }

    const button = document.createElement("button");
    button.type = "button";
    button.className = "unlock-box__go";
    button.textContent = shape.button;

    const problem = document.createElement("span");
    problem.className = "unlock-box__problem";
    if (spec.reason) {
        problem.textContent = spec.reason;
    }

    /** 解锁：先问后端，**读到读得动才**通知外面 */
    const reveal = async () => {
        button.disabled = true;
        // 旧的失败原因先清掉，不然重试成功之后它还杵在那里
        problem.textContent = "";
        try {
            const state = await lockState(
                spec.kind,
                spec.title,
                null,
                spec.needsPassphrase ? input.value : undefined,
            );
            if (!state.readable) {
                // 框留着、按钮还能按 —— 人改了口令（或者补了私钥）可以再试一次
                problem.textContent = state.reason || "还是读不出来";
                return;
            }
            await spec.onRevealed();
        } catch (error) {
            problem.textContent = String(error);
        } finally {
            button.disabled = false;
        }
    };

    button.addEventListener("click", () => void reveal());
    input.addEventListener("keydown", (event) => {
        if (event.key === "Enter") {
            event.preventDefault();
            void reveal();
        }
    });

    box.append(button, problem);
    return box;
}

/**
 * 把正文里后端摆下的 `span[data-decrypt]` 变成框。
 *
 * 后端只说"这里有个解锁框，它是这样"（`kind` / `title` / `label` / `reason`）——
 * 输入框、按钮、"显示"这三个动作由 [`decryptBox`] 造，理由见本文件抬头。
 *
 * 要不要给口令输入框**在这里问**，不在后端猜：后端拿到的那个 `Unreadable` 只说了
 * "读不出来"，没说是口令层还是 gpg 层。猜错的后果是**静默**的 —— gpg 的框上多一个
 * 输入框（人能看见），口令的框上没有（人卡住）。所以宁可多问一次。
 */
export function wireDecrypt(root: HTMLElement): void {
    for (const marker of root.querySelectorAll<HTMLElement>("[data-decrypt]")) {
        if (marker.dataset.decryptWired) {
            continue;
        }
        marker.dataset.decryptWired = "yes";

        const kind = marker.dataset.decryptKind === "file" ? "file" : "page";
        const title = marker.dataset.decryptTitle ?? "";
        const label = marker.dataset.decryptLabel || title;
        const reason = marker.dataset.decryptReason ?? "";

        void (async () => {
            let box: HTMLElement;
            try {
                const state = await lockState(kind, title);
                // 已经读得动了（别处解锁过了）：直接换掉，不摆一个没用的框
                if (state.readable) {
                    await revealed(kind, title);
                    return;
                }
                box = decryptBox({
                    kind,
                    title,
                    label,
                    needsPassphrase: state.needs_passphrase,
                    // 后端给的 `reason` 是"为什么当初读不出来"；`state.reason` 更准
                    // （它是刚才真读了一次得到的）。两个都有时听后者。
                    reason: state.reason || reason,
                    onRevealed: () => revealed(kind, title),
                });
            } catch (error) {
                box = decryptBox({
                    kind,
                    title,
                    label,
                    // 问不出来就退一步：不遮着要口令（gpg 不需要），但把原因摆出来
                    needsPassphrase: false,
                    reason: String(error),
                    onRevealed: () => revealed(kind, title),
                });
            }
            marker.replaceWith(box);
        })();
    }
}

/** 解锁成功了：模板页整篇重读（内容烤在渲染里），文件那一路由调用方自己换 */
function revealed(kind: LockKind, title: string): void {
    if (kind !== "page") {
        return;
    }
    rereadNote?.(title);
}