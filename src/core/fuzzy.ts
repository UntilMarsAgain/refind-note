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
 * 模糊搜索：按查询词给一批候选项排序。
 *
 * ## 这个文件是"模糊搜索怎么算"的**唯一**出处
 *
 * 调用方只认 [`fuzzySearch`] 这一个函数、只传"每项的标题"。打分、排���、
 * 一致性处理全在这里，所以**将来要换成库，只改这一个文件**。
 *
 * 我把它写成这样是有原因的：`AllPages.vue` 那个搜索框需要模糊匹配，
 * 但**并不确定以后会不会大量用上**。为一个还不确定的需求押上一个依赖不划算，
 * 而手写一份又怕散得到处都是 —— 于是把"边界"划在这里：
 * 需求真起来时，换 `fuse.js` 之类的库 = 重写这一个文件，
 * 调用方（一个 `computed`）一个字都不用改。
 *
 * ## 现在为什么够用
 *
 * 四档打分（完全匹配 > 前缀 > 包含 > 子序列），几十行，中文友好：
 * 中文没有大小写、没有词边界，子序列那一档按字符走就是对的。
 */

/**
 * 一个候选项：只要求能给出一个拿来比的字符串。
 *
 * 这里**故意不设索引签名**（`[key: string]: unknown`）：设了的话
 * `ListRow` 这类有具体字段的接口就传不进来（TS 报"缺索引签名"），
 * 而把结果原样交回去本来靠泛型 `T` 就够了，不需要索引签名。
 *
 * 于是约束就是一句话：`textOf` 能从它身上取出字符串。
 */
export type FuzzyText<T> = (item: T) => string;

/**
 * 打分档位。
 *
 * 从高到低 —— **同档内再按"匹配得紧"排**（见 [`score`]）。
 * 数值之间刻意留了空档（10/6/3/1），这样将来加中间档位不会打乱已有的次序。
 */
const EXACT = 10;
const PREFIX = 6;
const CONTAINS = 3;
const SUBSEQUENCE = 1;

/**
 * 给一项打分（`0` = 不匹配）。
 *
 * 逐档往下试，取**命中的最高那一档** —— 不是取"最长的那个子串"：
 * 查 `card` 时 `卡片` 有子序列匹配也有前缀匹配，前缀那一档更该赢。
 */
function score(haystack: string, needle: string): number {
    const text = haystack.toLowerCase();
    const want = needle.toLowerCase();

    if (text === want) {
        return EXACT;
    }
    if (text.startsWith(want)) {
        // 前缀匹配再按长度细分：越短越贴（`卡片` 该排在 `卡片模板` 前面）
        return PREFIX + (want.length / text.length) * 0.5;
    }

    const at = text.indexOf(want);
    if (at >= 0) {
        // 包含匹配也按位置细分：靠前的更可能是想要的
        return CONTAINS + (1 - at / text.length) * 0.5;
    }

    // 子序列：按顺序能对上就算（`kb` 能命中 `键盘笔记`）
    // 加一点"越紧凑越好"，否则 `笔记` 会排在 `我的笔记里的各种东西` 前面
    return isSubsequence(want, text) ? SUBSEQUENCE + (want.length / text.length) * 0.5 : 0;
}

/**
 * `needle` 的每个字符在 `haystack` 里按顺序都能找到吗。
 *
 * 从左往右扫，记住每个字符上一次出现在哪 —— 必须严格递增（否则 `ab` 能命中 `a…b…a…b`
 * 里的乱序，那是另一回事）。
 */
function isSubsequence(needle: string, haystack: string): boolean {
    let from = 0;
    for (const char of needle) {
        const at = haystack.indexOf(char, from);
        if (at < 0) {
            return false;
        }
        from = at + 1;
    }
    return true;
}

/**
 * 按查询词筛出并排序。
 *
 * @param query 用户敲的那串字
 * @param items 候选项
 * @param textOf 从一项里取出"拿来比的那个字符串"（一般就是标题）
 * @returns 命中的那些，**按相关度从高到低**；同分保持原次序（`sort` 是稳定的）
 */
export function fuzzySearch<T>(
    query: string,
    items: readonly T[],
    textOf: FuzzyText<T>,
): T[] {
    const want = query.trim();
    if (!want) {
        return [...items];
    }

    const scored: { item: T; rank: number; at: number }[] = [];
    items.forEach((item, at) => {
        const rank = score(textOf(item), want);
        if (rank > 0) {
            scored.push({ item, rank, at });
        }
    });

    // 同分按原次序：上面那个 `at` 就是干这个的。
    // 不这么排的话，同等相关度的项会随 `sort` 的实现乱跳 —— 那是"手滑"级别的 bug。
    scored.sort((a, b) => b.rank - a.rank || a.at - b.at);

    return scored.map((entry) => entry.item);
}