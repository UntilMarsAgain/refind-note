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
 * 数学公式：把 `.math[data-tex]` 交给 KaTeX 排版。
 *
 * **认出公式是后端的事**（`$…$`、`$$…$$`、`::math` 三种写法都落成
 * `<span class="math" data-tex="…">`），排版是这里的事 —— KaTeX 是 JS，
 * 后端不引 JS 引擎，而公式要跟着主题、字号、缩放进正文，本来就该在这一层落地。
 *
 * 没排版之前元素里显示的是**公式原文**，所以哪怕 KaTeX 没装上来，
 * 读到的也只是"原文"而不是一片空白。
 */

/* 字体与排版样式跟 JS 一起走：只在真的有公式时才需要（Vite 会把字体一起打进产物） */
import "katex/dist/katex.min.css";

type Katex = typeof import("katex").default;

let engine: Katex | null = null;
let loading: Promise<Katex | null> | null = null;

function load(): Promise<Katex | null> {
    if (engine) {
        return Promise.resolve(engine);
    }
    loading ??= import("katex")
        .then((module) => {
            engine = module.default;
            return engine;
        })
        .catch((error) => {
            console.warn("KaTeX 没装上来，公式按原文显示：", error);
            return null;
        });
    return loading;
}

/** 把这一片里的公式排出来（已经排过的跳过） */
export function renderMath(root: HTMLElement): void {
    const targets = [
        ...root.querySelectorAll<HTMLElement>(".math[data-tex]:not([data-math-ready])"),
    ];
    if (targets.length === 0) {
        return;
    }

    void load().then((katex) => {
        if (!katex) {
            return;
        }
        for (const element of targets) {
            // 取的时候是异步的：期间这一页可能已经被重新注入过了
            if (!element.isConnected) {
                continue;
            }
            element.dataset.mathReady = "yes";
            try {
                katex.render(element.dataset.tex ?? "", element, {
                    displayMode: element.classList.contains("math--display"),
                    // 写错了也照样排版：KaTeX 会把错处标红、**原样显示公式**，
                    // 比我们另画一个"公式有问题"的框更贴近作者写的东西
                    throwOnError: false,
                    // 公式是内容，不是代码：TeX 里的 \html* 之类一律不认
                    trust: false,
                });
            } catch (error) {
                // `throwOnError: false` 之后基本到不了这里；真到了，原文也还在
                console.warn("公式没排出来：", element.dataset.tex, error);
            }
        }
    });
}
