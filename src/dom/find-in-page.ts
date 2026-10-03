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
 * 阅读页的页内查找：在正文里把命中的那几段包起来高亮，能上一个、能下一个。
 *
 * ## 为什么用 `<mark class="find-hit">`，而且这个 class 是关键
 *
 * `<mark>` 在本程序里**有别的用途**：作者可以在 `::html` 里自己写标记，
 * 净化白名单也放它过去（`html.rs` 的 `RULES` 里有 `("mark", COMMON)`，
 * `COMMON` 含 `class`）。所以查找高亮与作者自己的标记会共处一个正文里。
 *
 * 因此**绝不能只靠样式区分**，也不能只认元素名：
 *
 * - 查找高亮是**临时状态** —— 换个查询就得全拆；
 * - 作者的标记是**内容** —— 拆了就没了。
 *
 * 于是定死一条规矩：**查找包出来的 `<mark>` 一律带 `find-hit`**，
 * 还原时**只拆带这个 class 的**。作者写的裸 `<mark>`、或挂着自己 class 的
 * （`mark.hl`、`mark.why` …）一个都不动。样式也只在 `.find-hit` 上写，
 * 不用裸 `mark` 选择器，所以作者的标记长什么样与查找无关。
 *
 * ## 为什么不碰 `code` / `pre`
 *
 * 代码块自带语法高亮，查找色叠在上面会变成两套颜色打架，而且一行里换行符、
 * 缩进那些**不可见字符**参与匹配时，命中位置与眼睛看到的位置对不上。
 * 正文里的行内 `code` 同理。找代码里的字另有那条路（编辑器的查找面板）。
 */

/** 查找高亮那一类 `<mark>` 的 class —— 只认它，绝不碰别的 `<mark>` */
export const FIND_HIT_CLASS = "find-hit";

/**
 * 当前那一个命中的 class（**状态**，与上面那个**序号**是两回事）。
 *
 * 序号写在 `data-find-hit` 上、永远不变；这个每次按"下一个"都换。
 * 分成两个是因为用途不同：序号用来定位，状态只给样式看
 * （`styles/note/find.css` 里 `.find-hit-selected` 加重边框）。
 */
export const CURRENT_CLASS = "find-hit-selected";

/** 挂在命中的序号上：点"上一个/下一个"按序号找，不靠 DOM 顺序猜 */
const HIT_ATTR = "data-find-hit";

/** 扫过但不该进去找的地方 */
const SKIP_SELECTOR = "code, pre, script, style, textarea, .find-hit";

/** 一次查找的结果：命中数与那些位置的容器 */
export interface FindResult {
    /** 命中总数 */
    total: number;
}

/**
 * 查找的选项（与浏览器查找的那两个开关同名同义）。
 *
 * 默认**都不开** —— 那是绝大多数人找东西时的期待：先找到，看见了再挑。
 */
export interface FindOptions {
    /** 区分大小写（默认否） */
    caseSensitive?: boolean;
    /** 只匹配整个词（默认否） */
    wholeWord?: boolean;
}

/**
 * 命中位置 `at` 那一段（长 `length`）是不是一个**整词**。
 *
 * 两边都得是词的边：左边那个字是词的延续（`Cat` 里的 `at` 不算），
 * 右边那个字是开头（`Cat` 里的 `Ca` 不算）。
 *
 * ## "词字"只认 `[A-Za-z0-9_]`，汉字不算 —— 这是**故意**跟浏览器一致
 *
 * 我第一版用 `\p{L}`（所有字母，含汉字）来判，结果**"整词"对中文几乎永远不命中**：
 * `中文的文` 里搜 `文`，前后都是汉字、都被当成"词的延续"，于是 0 处。
 *
 * 而浏览器（以及 `RegExp` 的 `\b`）用的 `词字` 是 `\w`，也就是
 * `[A-Za-z0-9_]` —— **汉字不在里面**，所以汉字之间处处是词的边，
 * 搜 `文` 就是 2 处。
 *
 * 两者对拉丁字母的行为完全一样，差别只在非拉丁文字上。既然这个开关是
 * 照着浏览器查找做的，就该跟它一样 —— 否则同一个正文，在浏览器里按"整词"
 * 找得到、在本程序里找不到。
 */
function isWordBoundary(text: string, at: number, length: number): boolean {
    const isWordChar = (ch: string | undefined): boolean =>
        ch !== undefined && /[A-Za-z0-9_]/.test(ch);
    const before = at > 0 ? text[at - 1] : undefined;
    const after = text[at + length];
    // 开头/结尾天然是词的边
    return !isWordChar(before) && !isWordChar(after);
}

/**
 * 拆掉上一次查找留下的高亮。
 *
 * 只拆 `find-hit`：**作者自己的 `<mark>` 原样留着**（见文件抬头那段）。
 *
 * 拆的方式是把 `mark` 的内容搬回它原来在的那个父节点，然后删掉 `mark` 本身 ——
 * 不是直接 `remove()`，那样连里面的字一起没了。
 */
export function clearFind(root: HTMLElement): void {
    // 先收集再改：边遍历边拆会让活着的 NodeList 出问题
    const marks = Array.from(root.querySelectorAll(`mark.${FIND_HIT_CLASS}`));
    for (const mark of marks) {
        const parent = mark.parentNode;
        if (!parent) {
            continue;
        }
        mark.classList.remove(CURRENT_CLASS);
        while (mark.firstChild) {
            parent.insertBefore(mark.firstChild, mark);
        }
        parent.removeChild(mark);
        // 拆完把相邻的文字节点并回去：拆过的地方会留下两个挨着的文本节点，
        // 不并的话下一次遍历会把同一段文字数两遍
        parent.normalize();
    }
}

/**
 * 在 `root` 里找 `query`，把每一处包成 `<mark class="find-hit">`。
 *
 * 先拆后找：换个查询时上一次的痕迹不能留在页面上（那些 `mark` 会把文字
 * 切成一段段，`TreeWalker` 走过去时片段边界也对不上了）。
 *
 * @param root 正文容器（`NoteContent` 里那个 `.note-body`）
 * @param query 要找的字；空串等于取消查找
 */
export function findIn(root: HTMLElement, query: string, options: FindOptions = {}): FindResult {
    clearFind(root);

    const needle = query.trim();
    if (!needle) {
        return { total: 0 };
    }

    const { caseSensitive = false, wholeWord = false } = options;
    // 一次性把该用哪种比较定下来：下面三处（筛文本节点、扫一遍、取长度）
    // 用的必须是**同一套**，否则会出现"扫到了却切不出那么长"的错位
    const wanted = caseSensitive ? needle : needle.toLowerCase();
    const text_ = (text: string): string => (caseSensitive ? text : text.toLowerCase());

    /** 这一处算不算命中 */
    const isHit = (text: string, at: number): boolean => {
        /*
         * 逐字比较，**不要用 `.match()`**。
         *
         * `.match(string)` 里的参数被当成**正则**，于是查询词 `cat` 变成
         * "含 c…a…t"，`hello` 变成"含任意 5 个字符" —— 命中数乱七八糟
         * （实测查 `cat` 只中 2 处而不是 4 处）。更糟的是像 `文` 这种
         * 查询，`/文/` 至少还能匹配，可 `*`、`.` 这类符号一旦被当正则，
         * 就会变成"匹配空串"，于是**每一处都成命中**。
         *
         * 查找框里什么字符都可能有人敲，所以必须当**字面量**处理。
         */
        const slice = text.slice(at, at + needle.length);
        if (text_(slice) !== wanted) {
            return false;
        }
        return wholeWord ? isWordBoundary(text, at, needle.length) : true;
    };

    // 收集文本节点时**先收集再改**：包 `mark` 会动 DOM，边走边包会漏掉或重复
    const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT, {
        acceptNode(node) {
            // 代码与脚本那些不参与（见文件抬头）
            if (node.parentElement?.closest(SKIP_SELECTOR)) {
                return NodeFilter.FILTER_REJECT;
            }
            const text = node.nodeValue ?? "";
            return text_(text).includes(wanted)
                ? NodeFilter.FILTER_ACCEPT
                : NodeFilter.FILTER_REJECT;
        },
    });

    const hits: { node: Text; index: number }[] = [];
    for (let node = walker.nextNode(); node; node = walker.nextNode()) {
        const text = node.nodeValue ?? "";
        let from = 0;
        for (;;) {
            const at = text_(text).indexOf(wanted, from);
            if (at < 0) {
                break;
            }
            // `indexOf` 找到的**不一定**合格（整词时尤其）：跳到下一处继续看
            if (!isHit(text, at)) {
                from = at + 1;
                continue;
            }
            hits.push({ node: node as Text, index: at });
            // 往后挪一个字符：同一个节点里可能有连着的两处命中
            from = at + Math.max(1, needle.length);
        }
    }

    // **同一个文本节点里的多处命中要倒着包**：包前面那处会把后面的文字
    // 切进新节点里，后面的 `index` 就对不上原来的那个节点了。
    // 所以按节点分组、组内从后往前。
    const byNode = new Map<Text, number[]>();
    for (const hit of hits) {
        const list = byNode.get(hit.node);
        if (list) {
            list.push(hit.index);
        } else {
            byNode.set(hit.node, [hit.index]);
        }
    }

    /**
 * 把每个节点里的命中包起来，并给它们**按文档顺序**编号。
 *
 * 两件事的顺序不能弄反：
 *
 * 1. **同一个节点内从后往前切**（`splitText` 会改动那个节点的文本，
 *    从前往后切的话后面那些 `index` 就不再指向原字符串里的位置了）；
 * 2. **编号按文档顺序发**，而不是按切的顺序。所以编号不在这层发，
 *    而是切完之后从前往后数一遍（`numberHits`）。
 *
 * 我第一版是在切的当场发编号，于是"从后往前切"直接把编号也倒过来了 ——
 * 界面上"1/3"落在最后一处高亮上，点"下一个"会跳到奇怪的位置。
 */
for (const [node, indexes] of byNode) {
    const length = needle.length;
    for (const index of [...indexes].sort((a, b) => b - a)) {
        const found = node.splitText(index);
        found.splitText(length);
        const mark = document.createElement("mark");
        mark.className = FIND_HIT_CLASS;
        found.parentNode?.insertBefore(mark, found);
        mark.appendChild(found);
    }
}

// 按文档顺序编号（`querySelectorAll` 返回的就是文档顺序）
const all = root.querySelectorAll(`mark.${FIND_HIT_CLASS}`);
all.forEach((mark, at) => mark.setAttribute(HIT_ATTR, String(at)));

return { total: all.length };
}

/** 命中的第 `index` 个（按下标取；越界返回 null） */
export function hitAt(root: HTMLElement, index: number): HTMLElement | null {
    const hits = root.querySelectorAll(`mark.${FIND_HIT_CLASS}`);
    return (hits[index] as HTMLElement | undefined) ?? null;
}

/** 命中的个数（界面上"3/12"那个分母） */
export function hitCount(root: HTMLElement): number {
    return root.querySelectorAll(`mark.${FIND_HIT_CLASS}`).length;
}

/**
 * 滚到第 `index` 个命中，并把它标成"当前这一个"。
 *
 * 当前那一个用 `find-hit-selected`（不是 `data-find-hit`）标出来 ——
 * 后者是**序号**（给定位用），这个是**状态**（给样式用）。
 * 分开是因为序号永远不变，而状态每次按"下一个"都要换。
 *
 * 滚不到就直接 `scrollIntoView`：本程序里可滚动的容器是外层
 * （`RenderPane` 的 `.pane`），`scrollIntoView` 会把整条链都带上，够用
 * （与 `ScrollTarget` 那种"算边界"的写法不同，这里不需要精确到行）。
 */
export function scrollToHit(root: HTMLElement, index: number): void {
    const hit = hitAt(root, index);
    if (!hit) {
        return;
    }
    // 先摘掉上一个的，再挂这一个：状态只有一个
    root.querySelector(`.${CURRENT_CLASS}`)?.classList.remove(CURRENT_CLASS);
    hit.classList.add(CURRENT_CLASS);
    hit.scrollIntoView({ block: "center", behavior: "smooth" });
}