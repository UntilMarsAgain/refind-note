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
 * 页内查找的状态：开着没有、查到几个、现在看第几个。
 *
 * ## 为什么查找状态在**外面**而不在 `NoteContent` 里
 *
 * 因为它有两个主人，各管一半：
 *
 * - 查找条（输入框、上下一个）是**界面**，得跟着窗口走；
 * - 那些高亮是**正文的一部分**，正文换一篇就得重做（`v-html` 会把内容整个换掉，
 *   高亮随之消失），视图被 KeepAlive 停用时也要收起来。
 *
 * 所以这一层只管"开着没开、现在第几个"，真正往 DOM 里包 `<mark>` 的动作
 * 交给 `dom/find-in-page.ts` —— 那层只碰 DOM，不认识组件。
 */

import { ref, type Ref } from "vue";
import { clearFind, findIn, scrollToHit } from "../dom/find-in-page.ts";

export interface FindInPage {
    /** 查找条开着没有 */
    open: Ref<boolean>;
    /** 命中的总数（"3/12"的分母） */
    total: Ref<number>;
    /** 现在是第几个（1 起；没命中时 0） */
    index: Ref<number>;
    /** 区分大小写（默认否） */
    caseSensitive: Ref<boolean>;
    /** 只匹配整词（默认否） */
    wholeWord: Ref<boolean>;
    /**
     * 正文容器：查找往里包 `<mark>`。
     *
     * 由视图把它接过来（`NoteContent` 用 `defineExpose` 交出 `rootEl`）。
     * 它变了、正文重画了，都要重做查找 —— 所以下面 watch 它。
     */
    root: Ref<HTMLElement | null>;
    /** 打开查找条（`find` 动作走这里） */
    show: () => void;
    /** 关掉：拆掉高亮、收起输入条 */
    hide: () => void;
    /** 输入变了：重查，并回到第一个 */
    search: (query: string) => void;
    /** 下一个（到末尾绕回第一个） */
    next: () => void;
    /** 上一个（到开头绕回最后一个） */
    previous: () => void;
    /** 改了选项：重查（回到第一个） */
    refresh: () => void;
}

export function useFindInPage(): FindInPage {
    const open = ref(false);
    const total = ref(0);
    const index = ref(0);
    const caseSensitive = ref(false);
    const wholeWord = ref(false);
    const root = ref<HTMLElement | null>(null);

    /** 重新查一遍并滚到第 `want` 个（0 起） */
    function run(want = 0) {
        const element = root.value;
        if (!element) {
            total.value = 0;
            index.value = 0;
            return;
        }
        const found = findIn(element, query, {
            caseSensitive: caseSensitive.value,
            wholeWord: wholeWord.value,
        });
        total.value = found.total;
        if (found.total === 0) {
            index.value = 0;
            return;
        }
        // 越界就绕回：到末尾再点"下一个"回到第一个，这是查找框的惯例
        const at = ((want % found.total) + found.total) % found.total;
        index.value = at + 1;
        scrollToHit(element, at);
    }

    /** 当前查询（存着是为了重做查找时用，比如正文换了） */
    let query = "";

    return {
        open,
        total,
        index,
        caseSensitive,
        wholeWord,
        root,

        show() {
            open.value = true;
        },

        hide() {
            open.value = false;
            query = "";
            total.value = 0;
            index.value = 0;
            // 拆掉高亮：关掉了还留着一片黄，那不是"关了"
            if (root.value) {
                clearFind(root.value);
            }
        },

        search(value) {
            query = value;
            run(0);
        },

        refresh() {
            // 没在找什么的时候切选项没有意义（`query` 是空的，跑了也是 0 处）
            if (!query) {
                return;
            }
            run(0);
        },

        next() {
            if (total.value === 0) {
                return;
            }
            run(index.value);
        },

        previous() {
            if (total.value === 0) {
                return;
            }
            run(index.value - 2);
        },
    };
}