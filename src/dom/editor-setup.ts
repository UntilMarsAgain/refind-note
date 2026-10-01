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
import type { DecorationSet, ViewUpdate } from "@codemirror/view";
import { Decoration, MatchDecorator, ViewPlugin } from "@codemirror/view";
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
        wikilinkHighlight,
        templateHighlight,
        scriptHighlight,
        templateFold,
    ];
}
