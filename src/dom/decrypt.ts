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
 * 页内解锁框 —— 模板页、正文那一篇、加密附件，三处共用。
 *
 * ## 规格（作者定的）
 *
 * 1. 后端遇到读不出来的东西，**先尝试解密**；
 * 2. 失败了就摆一个 `::decrypt` 占位（**只出标记**，见
 *    `markdown/syntax/template/dispatch.rs`）；
 * 3. 人在占位上提交 → 后端**真的去加载、解析**那一份；
 * 4. 成功 → 前端**替换掉这个占位**；失败 → 告诉前端（框留着，原因写在旁边）。
 *
 * 第 1 步"先尝试"是关键的：gpg 大部分时候是**自动**的（钥匙的口令有缓存、或者
 * 智能卡碰一下就行），所以不能一看见"加密"两个字就摆框 —— 那是给人白点一下。
 *
 * ## 为什么框只有这一个构造器
 *
 * 三条来路，而**信息到达的时间不一样**：正文那一篇与页内嵌着的模板页是渲染时
 * 就知道的（后端已经试过了，见上面第 1、2 步），附件是 `<img>` 加载失败之后才知道的。
 * 但它们都汇到 [`decryptBox`] —— 框的样子与"提交"这个动作只有一处定义。
 *
 * 钉它的形状靠两处测试：Rust 侧
 * `the_unlock_box_is_only_a_marker_so_both_callers_share_one_builder`
 * （后端不许自己造输入框与按钮），前端侧 `tests/web/decrypt.test.ts`。
 *
 * ## 为什么成功时不拿 HTML 来替换
 *
 * 因为三种上下文的"那一块"不是同一样东西：页内嵌着的模板页带着**调用点给的参数**
 * （`::卡片` 的参数只有重渲染才拿得回来），拿后端单独算的 HTML 去替换会把参数丢掉。
 * 所以后端只说"成不成"，替换由每处自己挑最对的做法（见 [`DecryptSpec.onRevealed`]）。
 */

import { type LockKind, resolveDecrypt } from "../ipc/lock.ts";

/** 摆一个框要知道的 */
export interface DecryptSpec {
    kind: LockKind;
    /** 显示标题（`unlock` 与 `read` 都按它走） */
    title: string;
    /** 给人看的名字 */
    label: string;
    /**
     * 要不要给口令输入框。
     *
     * **由后端给**，不由这里推断（`TemplatePage::Unreadable` 与 `Reading::Locked`
     * 都带着保护状态，后端刚读过那一页的封装头，它知道）。
     * 猜错是**静默**的：gpg 的框上多一个输入框（人白输一次），或者口令的框上
     * 没有（人卡在那儿）。
     */
    needsPassphrase: boolean;
    /** 已经知道的失败原因（探明之前渲染的场合才有） */
    reason?: string;
    /** 解锁成功了做什么 —— 每处自己挑最对的那一种，见本文件抬头 */
    onRevealed: () => void | Promise<void>;
}

/**
 * 框有哪几样 —— 由"要不要口令"**唯一**决定。
 *
 * 抽成纯函数是为了能测：这一段是 DOM 造型的源头，而 `node --test` 里没有 DOM。
 * 抽出来之后"gpg 的框不该有输入框"、"口令的框必须有"这两条就能被机器钉住，
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
        // 两句都说明**下一步**，不只是"这是加密的"——
        // 只说状态的话人还是不知道自己能干什么。
        hint: needsPassphrase ? "输入口令后显示" : "解锁后显示",
        // 两种都是"解锁"：口令层与 gpg 层做的是同一件事（让后端真的加载一次），
        // 只是那一次里问的是谁不一样。按钮上分两个字只会让人以为不是一件事。
        button: "解锁",
    };
}

/**
 * "解锁之后要把那一块换成什么"由 App 注入。
 *
 * 一件正事：**解锁成功之后到底刷新什么，每处不一样**，而这层（`dom/`）拿不到
 * 各处自己的做法。与 `note-html.ts` 的 `setOpenInNewTab` 同一套路：启动时由 App
 * 把一个"按种类分派"的实现塞进来。
 */
let revealByKind: ((kind: LockKind, title: string) => void | Promise<void>) | null = null;

/** App 启动时调用一次 */
export function setRevealDecrypted(handler: (kind: LockKind, title: string) => void | Promise<void>) {
    revealByKind = handler;
}

/**
 * 造一个解锁框，并把它按好了。
 *
 * ## 失败时框留在原地
 *
 * 这是整个模块最要紧的一条：`reveal` 先问后端（[`resolveDecrypt`]），**读到
 * `readable: true` 才通知外面**。反过来做（先换掉占位再看结果）的话，gpg 解密失败
 * 会变成"图片不存在"而且**无法重试** —— 框已经没了。
 */
export function decryptBox(spec: DecryptSpec): HTMLElement {
    // 后端可能换掉"要不要口令"（这一版既对称又 gpg），所以框的状态是自己的局部量，
    // 不写回 `spec` —— 那是调用方的对象，它可能被复用（重新 `innerHTML` 时会再走一遍）
    let needsPassphrase = spec.needsPassphrase;

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
    const shape = boxShape(needsPassphrase);

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

    /** 提交：问后端"真的加载一次"成不成 */
    const reveal = async () => {
        button.disabled = true;
        // 旧的失败原因先清掉，不然重试成功之后它还杵在那里
        problem.textContent = "";
        try {
            const result = await resolveDecrypt(
                spec.kind,
                spec.title,
                null,
                needsPassphrase ? input.value : undefined,
            );
            if (!result.readable) {
                // 框留着、按钮还能按 —— 人改了口令（或者补了私钥、碰了智能卡）可以再试
                problem.textContent = result.reason || "还是读不出来";
                return;
            }
            // 后端说这一版还要口令（这一版既对称又 gpg，外层解开后里面还要问）：
            // 把输入框补上。少这一步的话人会看到一个"解锁"按钮，按了又被要口令。
            if (result.needs_passphrase && !needsPassphrase) {
                needsPassphrase = true;
                if (!input.parentElement) {
                    box.insertBefore(input, button);
                }
                hint.textContent = boxShape(true).hint;
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
 * 后端只说"这里有个解锁框，它是这样"（`kind` / `title` / `label` / `reason` /
 * `needs-passphrase`），输入框与按钮由 [`decryptBox`] 造 —— 理由见本文件抬头。
 *
 * **这里不再问后端"要不要口令"**：标记里已经写了。后端在渲染的时候刚读过那一页的
 * 封装头，它知道；再问一次既是白跑一趟，也会让人以为那个判断有两处。
 */
export function wireDecrypt(root: HTMLElement): void {
    for (const marker of root.querySelectorAll<HTMLElement>("[data-decrypt]")) {
        if (marker.dataset.decryptWired) {
            continue;
        }
        marker.dataset.decryptWired = "yes";

        const kind: LockKind = marker.dataset.decryptKind === "file" ? "file" : "page";
        const title = marker.dataset.decryptTitle ?? "";
        const label = marker.dataset.decryptLabel || title;

        marker.replaceWith(
            decryptBox({
                kind,
                title,
                label,
                needsPassphrase: marker.dataset.decryptNeedsPassphrase === "yes",
                reason: marker.dataset.decryptReason ?? "",
                onRevealed: () => revealByKind?.(kind, title),
            }),
        );
    }
}