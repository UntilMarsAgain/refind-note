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
 * 数一数写了多少：字符、字、行。
 *
 * ## 为什么不算成 `text.length`
 *
 * 那是 **UTF-16 码元**的个数：一个 emoji 占两个（🧭 数成 2），
 * 而代理对之外的组合符号还会更离谱。屏幕上"我写了多少"要的是**看得见的字**，
 * 所以按码点走（`[...text]`）。
 *
 * ## 「字」是怎么数的
 *
 * 中日韩字符**一个字就是一字**（`水文` 是 2），而拉丁字母与数字是**连着算**
 *（`refind` 是 1 —— 否则一个英文单词要数成六个"字"，那个数字没有意义）。
 * 两者之外的一切（标点、符号、空白）都不计。
 *
 * 分档而不是给一个数，是因为"这一篇多长"这个问题本来就有两种答案：
 * 混排的笔记里两个数都不难看，纯中文的笔记里"字数"才是那个有用的。
 */

/** 一篇文本的三个数 */
export interface TextCount {
    /** 字符数：不算空白（换行、缩进不算"写了字"） */
    chars: number;
    /** 字数：中日韩一字一算，拉丁与数字连着算 */
    words: number;
    /** 行数：空文本算 0 行，有内容时至少 1 行 */
    lines: number;
}

/**
 * 是不是该**一字一算**的字符。
 *
 * 收的是"**写出来就是一个字**"的那些：CJK 表意文字、假名、韩文谚文音节 ——
 * 谚文音节虽然是由两个字母组合出来的，但**写**出来就是一个字，一字一算才对。
 *
 * 假名也是一字一算（一串假名读两遍，但那是"读"的事；这里数的是"写了几个字"）。
 *
 * 刻意**不做**音节级的分词（`Intl.Segmenter` 能把日文长音、泰文声调都算对）——
 * 它在很老的 webview 上没有，而这一栏是**随手看一眼**的数字：
 * 宁可给一个粗略而稳定的数，也不要一个时有时无的数。
 */
function isCJK(code: number): boolean {
    return (
        // CJK 统一表意文字（含扩展 A 与兼容表意文字）
        (code >= 0x4e00 && code <= 0x9fff) ||
        (code >= 0x3400 && code <= 0x4dbf) ||
        (code >= 0xf900 && code <= 0xfaff) ||
        // 假名
        (code >= 0x3040 && code <= 0x30ff) ||
        // 谚文音节
        (code >= 0xac00 && code <= 0xd7af)
    );
}

/** 算不算"连着算一个词"的字符：拉丁字母、数字，以及词内会用到的几个 */
function isWordPart(char: string): boolean {
    return /[\p{L}\p{N}_'’-]/u.test(char) && !isCJK(char.codePointAt(0) ?? 0);
}

/**
 * 数一数。
 *
 * @param text 整篇正文（编辑器里那份源码）
 */
export function countOf(text: string): TextCount {
    if (!text) {
        return { chars: 0, words: 0, lines: 0 };
    }

    let chars = 0;
    let words = 0;
    let inWord = false;

    // 按码点走（不是按码元）：一个 emoji 就是一个字符
    for (const char of text) {
        if (/\s/u.test(char)) {
            chars += 0; // 空白不算"写了字"，但它会**断词**
            inWord = false;
            continue;
        }
        chars += 1;
        if (isCJK(char.codePointAt(0) ?? 0)) {
            words += 1;
            inWord = false;
        } else if (isWordPart(char)) {
            // 连续的一段算一个词；断在空白或标点上
            if (!inWord) {
                words += 1;
                inWord = true;
            }
        } else {
            // 标点与符号：不计，也不该把一个词粘起来
            inWord = false;
        }
    }

    return { chars, words, lines: text.split("\n").length };
}

/** 给状态行用的一句话（"甲乙 12 字符 · 8 字"那种） */
export function describeCount(count: TextCount): string {
    return `${count.chars} 字符 · ${count.words} 字`;
}