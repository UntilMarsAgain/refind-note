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
 * 编辑与**查看**源码共用的 CodeMirror 配置。
 *
 * 两处用的是同一套东西：配色接项目的 token、markdown 的解析口径与后端对齐、
 * `[[内部链接]]` 与模板块的装饰、以及按语言给代码块上色。分开写两份的话，
 * "编辑器里看到的样子"与"查看源码时看到的样子"迟早会对不上 —— 而这两处本来就该一样。
 *
 * 各组件自己加的是**行为**：编辑器加自动保存与粘贴上传，查看那边加只读。
 */

import { EditorView } from "@codemirror/view";
import { markdown as markdownLanguage } from "@codemirror/lang-markdown";
import { javascriptLanguage } from "@codemirror/lang-javascript";
import { HighlightStyle, foldService, syntaxHighlighting } from "@codemirror/language";
import { highlightTree, tags } from "@lezer/highlight";
import type { Extension } from "@codemirror/state";
import { EditorState, Prec } from "@codemirror/state";
import type { DecorationSet, ViewUpdate } from "@codemirror/view";
import { Decoration, keymap, MatchDecorator, ViewPlugin } from "@codemirror/view";
import { openSearchPanel, search } from "@codemirror/search";
import { resolvedTheme } from "../core/theme.ts";
import {
    scriptRanges,
    templateBlockLines,
    templateFoldRange,
    templateRanges,
} from "../core/markdown/template-blocks.ts";

/**
 * CodeMirror 的配色**全部接项目的 token**，所以它跟着主题与主题色走，不必写两套样式。
 *
 * 为什么必须显式给：CM6 自带的 `basicSetup` 是按**浅色底**配的（正文近黑、高亮偏暗），
 * 铺在深色主题上就是"浅色面板 + 浅色文字"，看不清。这里做两件事：
 * 1. `EditorView.theme` 覆盖界面色（背景、正文、光标、选区、行号、当前行、提示框）；
 * 2. `syntaxHighlighting` 换掉默认高亮，改用 token 里的 `--syntax-*` 系列 ——
 *    与正文代码块同一套颜色。
 *
 * 值全是 `var(--…)`，所以切换深浅主题、甚至改主题色都不需要重新配置编辑器。
 */
export const appTheme = EditorView.theme(
    {
        "&": { color: "var(--text)", backgroundColor: "transparent", height: "100%" },
        ".cm-content": { caretColor: "var(--accent)", fontFamily: "var(--mono-font)" },
        /* 限高加在 CM6 自己的滚动容器上：滚外层会让它的虚拟渲染算错可视范围 */
        ".cm-editor": { height: "auto" },
        ".cm-scroller": { maxHeight: "calc(100vh - 240px)", overflow: "auto" },
        ".cm-cursor, .cm-dropCursor": { borderLeftColor: "var(--accent)" },
        "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection":
            { backgroundColor: "var(--selection-bg)" },
        ".cm-gutters": {
            backgroundColor: "transparent",
            color: "var(--text-dim)",
            border: "none",
        },
        ".cm-activeLine": { backgroundColor: "var(--hover)" },
        ".cm-activeLineGutter": { backgroundColor: "var(--hover)" },
        ".cm-panels": { backgroundColor: "var(--surface)", color: "var(--text)" },
        ".cm-tooltip": {
            backgroundColor: "var(--surface)",
            color: "var(--text)",
            border: "1px solid var(--border)",
        },
        ".cm-tooltip-autocomplete > ul > li[aria-selected]": {
            backgroundColor: "var(--hover)",
            color: "var(--text)",
        },
        // 自定义的 [[内部链接]] 与模板块装饰（CM6 生成的元素不在 scoped 作用域里，只能写在这里）
        // 模板块：头行看得出是"一次模板调用"，块内的行淡染一层，边界一目了然
        ".cm-template-head": {
            color: "var(--syntax-keyword)",
            fontWeight: "600",
        },
        ".cm-template-body": {
            // 只盖住行首缩进：那是"这段属于这个块"的标记，别染到正文
            backgroundColor: "var(--accent-tint)",
        },
        // 块的左边缘（整行装饰）：空行也在内，所以跨空行是**连续**的一条
        ".cm-template-line": {
            boxShadow: "inset 3px 0 0 0 var(--accent-tint)",
        },
        ".cm-wikilink": {
            color: "var(--accent)",
            borderBottom: "1px dotted var(--accent)",
        },
    },
    {
        // 用解析后的深浅色：显式选深色就是深色，"跟随系统"已经在这里解析过了
        dark: resolvedTheme.value === "dark",
    },
);

/**
 * 查找与替换面板。
 *
 * `basicSetup` 里**已经有 `searchKeymap`**（Ctrl+F / Ctrl+H / Ctrl+G 都绑好了），
 * 唯独没装 `search()` 本身 —— 键是按的，面板不开。现在把它装上。
 *
 * `top: true` 让面板贴在编辑器**顶上**：默认是沉在底下，而源码栏下面还有状态栏，
 * 沉底容易被那一行挡住，也离视线更远。
 *
 * ## 为什么要自己拿掉 `Mod-f`
 *
 * 全局的 Ctrl+F 现在归**键位表**管（见 `core/keymap.ts`），而 CM6 自带的
 * `Mod-f` 打开的是它自己那套面板 —— 用户改了键（比如改成 `Alt+K`）之后，
 * 按新键走我们的、按 Ctrl+F 走它的，同一个功能两个入口两套行为。
 *
 * 所以从 `basicSetup` 的键位表里把 `Mod-f` 摘掉，**只留替换与查找下一个**
 * （`Mod-h` / `Mod-g` 是"面板开着时"才用的，不冲突）。
 * `search()` 提供的 `openSearchPanel` 仍在，摘掉的是键位不是命令。
 */

/**
 * 编辑器里的查找：面板 + 高亮 + 键位（我们自己的键位表那份另算，见
 * `App.vue` 的 `onShortcut` —— 它管的是**页面**上的查找，编辑器里这个是
 * 编辑器自己的事，两处各按各的上下文）。
 *
 * 高亮颜色接项目的 token，所以跟着主题与主题色走：
 * - 全部命中 `var(--selection-bg)` 的淡染（它本来就表示"选中"）；
 * - 当前那个命中 `var(--accent-tint)`，再配一条强调色的边 —— 几十处命中时，
 *   一眼要能认出"现在看的是哪一个"，否则"下一个"按了等于没按。
 */
const editorSearch = [
    // 面板上的字全是英文 —— CM6 有官方的翻译机制（`EditorState.phrases`），用它，
    // 别去改 DOM。
    //
    // **键名必须与 `@codemirror/search` 里 `phrase(view, "…")` 的字面量一字不差。**
    // 我上一版照着自己看到的补，漏了两个（实测出来的）：三个勾选项的标签是
    // `"match case"` / `"by word"` / `"regexp"`（**带空格、全小写**），
    // 而按钮是 `"next"` / `"previous"` / `"replace all"` / `"Replace"`。
    // 猜错了不报错，只是那一条悄悄保持英文 —— 所以下面这段是照着源码核过的。
    EditorState.phrases.of({
        // —— 输入框的占位与名字
        Find: "查找",
        Replace: "替换",
        // —— 三个勾选项：键名带空格、全小写
        "match case": "区分大小写",
        regexp: "正则",
        "by word": "整词匹配",
        // —— 按钮：`select` 那颗的键是 `"all"`（选中全部命中），`replace` 是
        //    `"replace"`（替换这一个），别与 `"replace all"` 混了 ——
        //    三个键都在，但它们是三颗不同的按钮。
        next: "下一个",
        previous: "上一个",
        all: "全选匹配",
        replace: "替换",
        "replace all": "全部替换",
        close: "关闭",
    }),
    search({ top: true }),
    // `Mod-f` 让给键位表（见上面那段说明）。
    //
    // 注意这里**不是**"再绑一次盖住它"：CM6 里两条同键的键位不会互相抵消 ——
    // `basicSetup` 那条照样命中，面板照样开。要让某个键**什么都不发生**，
    // 唯一可靠的办法是用最高优先级绑一个返回 true 的空动作把它吃掉。
    //
    // 空动作里什么都不做（连查找面板也不开）：真正的"打开查找"由全局键位表
    // 那条 `find` 动作负责，它知道该开哪个查找 —— 页面上的那个，或者是编辑器里这个。
    // `keymap.of` 只收一个参数，优先级靠 `Prec` 套一层
    Prec.highest(keymap.of([{ key: "Mod-f", run: () => true }])),
    // 查找命中的配色。
    //
    // 必须用 **`&`** 起头（`"& .cm-searchMatch"`）而不是 `".cm-searchMatch"`：
    // `search()` 自己带了一整套浅色/深色默认（浅色底下是荧光黄、深色底下是荧光青），
    // 而它是 `EditorView.baseTheme` 的一部分。我们的 `appTheme` 排在它后面，
    // 选择器写对了才盖得住 —— 写错了就是"两套配色打架"，实测里我一开始就踩了这个。
    //
    // 类名是查过源码的：CM 用的是 `cm-searchMatch` 与
    // `cm-searchMatch cm-searchMatch-selected`（见 `@codemirror/search` 的 `selectedMatchMark`）。
    EditorView.theme({
        "& .cm-searchMatch": {
            backgroundColor: "var(--accent-tint)",
            outline: "1px solid var(--accent-soft)",
            borderRadius: "2px",
        },
        "& .cm-searchMatch-selected": {
            // 不写 `var(--selection-bg)` 当底色：那个变量是给**选中**用的，
            // 拿来做"当前这一个"在浅色底下几乎看不出差别。这里靠更重的边框认它 ——
            // 几十处命中时，一眼要能认出"现在看的是哪一个"。
            outline: "2px solid var(--accent)",
        },
        "& .cm-panel input, & .cm-panel button": {
            fontFamily: "var(--mono-font)",
        },
        /*
         * 下面这组是把 CM6 默认那套面板**收拾成这个程序的样子**。
         *
         * 默认布局是"一行挤到底"：两个输入框 + 五个按钮 + 三个勾选项全在同一行，
         * 输入框只有 60px 宽，右上那个 `×` 离关闭键十万八千里。中文更挤 ——
         * "区分大小写"四个字比 `match case` 还宽，于是整条面板挤作一团。
         *
         * 所以这里改的是**排版**，不改功能：面板仍由 CM6 生成（功能、快捷键、
         * 状态都是它的），我们只给它穿衣服。
         */
        // 面板本身：`padding-right` 留出关闭键那条，不与它抢位置
        "& .cm-panels": {
            padding: "8px 34px 8px 10px",
            backgroundColor: "var(--surface)",
            color: "var(--text)",
            borderBottom: "1px solid var(--border)",
            fontFamily: "var(--sans-font)",
            fontSize: "12.5px",
        },
        "& .cm-panels.cm-panels-top": {
            // 面板在顶上时别跟编辑器内容贴死，留一道缝
            borderBottom: "1px solid var(--border)",
        },
        "& .cm-search": {
            display: "flex",
            flexWrap: "wrap",
            gap: "8px 10px",
            alignItems: "center",
            // `max-width` 是必要的：不封顶的话输入框在宽窗口里会拉成一条长带
            maxWidth: "720px",
        },
        /*
         * 输入框：第一行那个"查找"是主体，给足宽度；
         * 第二个"替换"是次要的，窄一档就够（CM6 用 `name` 区分）。
         */
        "& .cm-search input[name=search], & .cm-textfield[name=search]": {
            flex: "1 1 220px",
            minWidth: "180px",
            height: "28px",
            padding: "0 8px",
            border: "1px solid var(--border)",
            borderRadius: "5px",
            backgroundColor: "var(--field-bg)",
            color: "var(--text)",
        },
        "& .cm-search input[name=replace], & .cm-textfield[name=replace]": {
            flex: "1 1 160px",
            minWidth: "130px",
            height: "28px",
            padding: "0 8px",
            border: "1px solid var(--border)",
            borderRadius: "5px",
            backgroundColor: "var(--field-bg)",
            color: "var(--text)",
        },
        "& .cm-search input:focus": {
            outline: "none",
            borderColor: "var(--accent-soft)",
        },
        // 按钮：CM6 默认是"灰底 + 渐变图片"，深色底下看着像块脏斑
        "& .cm-button": {
            display: "inline-flex",
            alignItems: "center",
            height: "28px",
            padding: "0 10px",
            border: "1px solid var(--border)",
            borderRadius: "5px",
            backgroundColor: "transparent",
            backgroundImage: "none",
            color: "var(--text-dim)",
            fontFamily: "var(--sans-font)",
            fontSize: "12.5px",
            cursor: "pointer",
        },
        "& .cm-button:hover": {
            backgroundColor: "var(--hover)",
            color: "var(--text)",
        },
        // 按钮之间的缝：CM6 把它们当行内元素排，之间没有间距
        "& .cm-search br + .cm-button": {
            marginLeft: "2px",
        },
        // 关闭键：钉在面板右上角，别让它跟在按钮后面跑到天边
        "& .cm-button[name=close]": {
            position: "absolute",
            top: "6px",
            right: "8px",
            width: "24px",
            height: "24px",
            padding: "0",
            border: "none",
            borderRadius: "5px",
            fontSize: "16px",
            lineHeight: "1",
        },
        // 勾选项：默认的勾选框是系统色，跟着主题走不了
        "& .cm-search label": {
            display: "inline-flex",
            alignItems: "center",
            gap: "4px",
            color: "var(--text-dim)",
            cursor: "pointer",
            userSelect: "none",
        },
        "& .cm-search label input[type=checkbox]": {
            accentColor: "var(--accent)",
            cursor: "pointer",
        },
        // 面板是"浮在编辑器上面"的，所以给它自己的定位上下文（`×` 要 absolute）
        "&.cm-focused .cm-panels": {
            position: "relative",
        },
    }),
];

/**
 * 与后端渲染器对齐的 markdown 解析。
 *
 * 后端关掉了两种写法，编辑器这边必须跟着关，否则**高亮会说谎**：
 *
 * - Setext 标题（`标题` 下一行写 `===` 或 `---`）：后端已不再把它解析成标题；
 * - 缩进代码块（四空格开头）：那个缩进后端留给了**模板块的边界**。
 *
 * 不关的后果很具体：编辑器把一段文字画成标题或代码，渲染出来却是普通段落。
 */
const syncedMarkdown = () =>
    markdownLanguage({ extensions: [{ remove: ["SetextHeading", "IndentedCode"] }] });

export const appHighlight = HighlightStyle.define([
    { tag: tags.heading, color: "var(--syntax-title)", fontWeight: "600" },
    { tag: tags.strong, color: "var(--text)", fontWeight: "600" },
    { tag: tags.emphasis, color: "var(--text)", fontStyle: "italic" },
    { tag: tags.link, color: "var(--accent)", textDecoration: "underline" },
    { tag: tags.url, color: "var(--accent)" },
    { tag: tags.monospace, color: "var(--syntax-string)" },
    { tag: tags.quote, color: "var(--syntax-comment)" },
    { tag: tags.list, color: "var(--syntax-number)" },
    { tag: tags.contentSeparator, color: "var(--border)" },
    { tag: tags.processingInstruction, color: "var(--syntax-keyword)" },

    // ---- 代码语言的 token ----
    // markdown 之外（代码围栏里的 css / html 等）会产出另一套 tag，这里按同一套
    // `--syntax-*` 变量补齐，免得"语言配上了却没有颜色"。
    { tag: tags.comment, color: "var(--syntax-comment)", fontStyle: "italic" },
    { tag: tags.string, color: "var(--syntax-string)" },
    { tag: tags.number, color: "var(--syntax-number)" },
    { tag: [tags.bool, tags.null], color: "var(--syntax-number)" },
    { tag: tags.keyword, color: "var(--syntax-keyword)" },
    { tag: [tags.controlKeyword, tags.operatorKeyword, tags.definitionKeyword, tags.modifier],
        color: "var(--syntax-keyword)" },
    // 标签名读起来像关键字；属性名像标题
    { tag: tags.tagName, color: "var(--syntax-keyword)" },
    { tag: tags.attributeName, color: "var(--syntax-title)" },
    { tag: tags.propertyName, color: "var(--syntax-title)" },
    { tag: tags.attributeValue, color: "var(--syntax-string)" },
    { tag: [tags.className, tags.typeName], color: "var(--syntax-type)" },
    { tag: [tags.atom, tags.constant(tags.variableName)], color: "var(--syntax-number)" },
    // 括号、运算符之类是结构，压低存在感，别和内容抢
    { tag: [tags.operator, tags.punctuation, tags.bracket, tags.separator, tags.angleBracket],
        color: "var(--text-dim)" },
    { tag: tags.invalid, color: "var(--syntax-deleted)" },
]);

/**
 * `[[内部链接]]` 的高亮。
 *
 * CM6 的 markdown 语法并不认识它（那是本项目的扩展语法），所以在**视图层**加装饰：
 * 只加样式、不改文档，保存下来的仍然是原文，渲染依旧由后端负责 —— 编辑器不做第二套解析。
 *
 * 匹配的是整个 `[[…]]`，所以里面无论写标题还是 `名称#章节`，都会被一起标出来
 * （地址的识别本就在这一对方括号里）。
 */
/**
 * `::js`（与 `::code lang=js`）块里的 JavaScript 上色。
 *
 * markdown 的语法树里，模板块的正文只是一段普通文字 —— 编辑器不认识里面写的是什么，
 * 于是 `::js` 的正文是一片没有颜色的字，写起来最容易出错的就是这种地方。
 *
 * 做法：把块内正文**单独**交给 JS 解析器解析一遍，再用**同一个** `appHighlight`
 * 把 token 翻成 class（`highlightTree` 产出的正是 `syntaxHighlighting(appHighlight)`
 * 用的那套类名，所以颜色与围栏代码块是同一套，不必再配一遍）。
 * 只加装饰、不改文档：保存下来的仍是原文，渲染仍由后端负责。
 */
function buildScriptDecorations(view: EditorView): DecorationSet {
    const lines = view.state.doc.toString().split("\n");
    const items: { from: number; to: number; decoration: Decoration }[] = [];

    for (const block of scriptRanges(lines)) {
        const tree = javascriptLanguage.parser.parse(block.code);
        highlightTree(tree, appHighlight, (from, to, classes) => {
            items.push({
                from: block.from + from,
                to: block.from + to,
                decoration: Decoration.mark({ class: classes }),
            });
        });
    }

    items.sort((a, b) => a.from - b.from || a.to - b.to);
    return Decoration.set(
        items.map((item) => item.decoration.range(item.from, item.to)),
        true,
    );
}

const scriptHighlight = ViewPlugin.fromClass(
    class {
        decorations: DecorationSet;
        constructor(view: EditorView) {
            this.decorations = buildScriptDecorations(view);
        }
        update(update: ViewUpdate) {
            if (update.docChanged || update.viewportChanged) {
                this.decorations = buildScriptDecorations(update.view);
            }
        }
    },
    { decorations: (plugin) => plugin.decorations },
);

const wikilinkMatcher = new MatchDecorator({
    regexp: /\[\[[^\]\n]+\]\]/g,
    decoration: Decoration.mark({ class: "cm-wikilink" }),
});

const templateHighlight = ViewPlugin.fromClass(
    class {
        decorations: DecorationSet;
        constructor(view: EditorView) {
            this.decorations = buildTemplateDecorations(view);
        }
        update(update: ViewUpdate) {
            if (update.docChanged || update.viewportChanged) {
                this.decorations = buildTemplateDecorations(update.view);
            }
        }
    },
    { decorations: (plugin) => plugin.decorations },
);

/**
 * 模板块的高亮。
 *
 * **算什么装饰在 `template-blocks.ts`**（纯函数，与后端逐条一致）；
 * 这里只负责把"行号 + 列"换成 CM6 的文档偏移量。
 */
function buildTemplateDecorations(view: EditorView): DecorationSet {
    const doc = view.state.doc;
    const lines = doc.toString().split("\n");
    const items: { from: number; to: number; decoration: Decoration }[] = [];

    // 标记：整行（头行）或行首缩进（块内）
    for (const range of templateRanges(lines)) {
        items.push({
            from: range.from,
            to: range.to,
            decoration: Decoration.mark({
                class: range.head ? "cm-template-head" : "cm-template-body",
            }),
        });
    }

    // 整行装饰：块的左边缘。**空行也要** —— 空行没有列可以上色，
    // 只靠标记的话块会在空行处断开一条缝，看上去像"没跨过空行"（其实跨过了）。
    for (const index of templateBlockLines(lines)) {
        const line = doc.line(index + 1);
        items.push({
            from: line.from,
            to: line.from,
            decoration: Decoration.line({ class: "cm-template-line" }),
        });
    }

    items.sort((a, b) => a.from - b.from || a.to - b.to);
    return Decoration.set(
        items.map((item) => item.decoration.range(item.from, item.to)),
        true,
    );
}

/**
 * 模板块的折叠。
 *
 * CM6 默认按 markdown 的结构折（段落、标题…），而模板块对它只是一段普通文字 ——
 * 折到第一个空行就停了，与渲染的"跨空行"对不上。这里按**同一套规则**给出范围：
 * 头行到块的最后一行（含块内空行）。
 */
const templateFold = foldService.of((state, lineStart) => {
    const doc = state.doc;
    const line = doc.lineAt(lineStart);
    const range = templateFoldRange(doc.toString().split("\n"), line.number - 1);
    if (!range) {
        return null;
    }
    return { from: line.to, to: doc.line(range.to + 1).to };
});

const wikilinkHighlight = ViewPlugin.fromClass(
    class {
        decorations: DecorationSet;

        constructor(view: EditorView) {
            this.decorations = wikilinkMatcher.createDeco(view);
        }

        update(update: ViewUpdate) {
            this.decorations = wikilinkMatcher.updateDeco(update, this.decorations);
        }
    },
    { decorations: (plugin) => plugin.decorations },
);

/**
 * 一份源码该有的全部装饰与配色（`basicSetup` 之外的那些）。
 *
 * 编辑与查看都从这里取 —— 唯一的差别是查看那边另外加上"只读"。
 */
export function sourceExtensions(): Extension[] {
    return [
        // 顺序有讲究：主题与高亮都要排在 basicSetup **之后**，才能盖掉它的浅色默认值 ——
        // 所以调用方要把这一组接在 `basicSetup` 后面
        syncedMarkdown(),
        appTheme,
        syntaxHighlighting(appHighlight),
        ...editorSearch,
        wikilinkHighlight,
        templateHighlight,
        scriptHighlight,
        templateFold,
    ];
}

// ----------------------------------------------------- 活着的编辑器实例

/**
 * 当前活着的编辑器实例（后建的在前）。
 *
 * 为什么需要这份登记：全局那个 `find` 动作是从 `window` 上的按键发起的，
 * 手里只有一个"动作 id"，拿不到"该对哪个编辑器开面板"。而
 * `openSearchPanel` 是个 `Command`，要的就是一个 `view`。
 *
 * 不用全局变量硬存一个"当前那个"：同时可能有两个编辑器（编辑态左右两个窗格，
 * 见 `EditorPanes.vue`），"当前"取决于焦点在哪，而焦点我们不跟踪 —— 所以登记
 * 全部，取**有焦点**的那个，没有就取最近一个。
 */
const liveViews: EditorView[] = [];

/** 编辑器建好时登记 */
export function registerEditorView(view: EditorView): void {
    liveViews.unshift(view);
}

/** 编辑器拆掉时销号（不销号就会一直指着已经 `destroy()` 的实例） */
export function unregisterEditorView(view: EditorView): void {
    const at = liveViews.indexOf(view);
    if (at >= 0) {
        liveViews.splice(at, 1);
    }
}

/** 眼下该对哪个编辑器动手：有焦点的那个，没有就最近一个；一个都没有返回 null */
export function focusedEditorView(): EditorView | null {
    const focused = liveViews.find((view) => view.hasFocus);
    return focused ?? liveViews[0] ?? null;
}

/**
 * 打开编辑器的查找面板（全局 `find` 动作在**编辑器里**时走这里）。
 *
 * 一个编辑器都没有就返回 false —— 调用方据此知道"这里没东西可找"，
 * 该去页面上找，而不是把面板开在空气上。
 */
export function openEditorSearch(): boolean {
    const view = focusedEditorView();
    if (!view) {
        return false;
    }
    // `openSearchPanel` 本身就是 `Command`（`(view) => boolean`），不是带 `.run` 的对象
    openSearchPanel(view);
    view.focus();
    return true;
}
