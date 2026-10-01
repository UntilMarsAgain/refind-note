/**
 * mermaid 图：把 `<pre class="mermaid" data-source="…">` 画成 SVG。
 *
 * 后端只把**原文**放进 `<pre class="mermaid">`（见 `syntax/template/stdlib.rs` 的
 * `render_mermaid`），画图在这里做。三件事要记住：
 *
 * 1. mermaid 画完会把元素内容换成 SVG —— 原文另存一份在 `data-source` 上，
 *    切主题、这一页重新注入时拿它再放回去；
 * 2. **配色是画的时候定死的**，所以切主题要把已经画过的整体重画；
 * 3. 画不出来（语法错）就把原文留着，旁边标一下 —— 别把一个空框留在正文里。
 *
 * 编辑时每次按键都会重新注入 HTML，所以这里**攒一下**再画（见 [`scheduleDiagrams`]）：
 * 不然每敲一个字都要重画一遍图，打字会卡。
 */

type Mermaid = typeof import("mermaid").default;

let engine: Mermaid | null = null;
let loading: Promise<Mermaid | null> | null = null;
/** 上一次画的时候用的是哪套主题（换主题要重画） */
let drawn: "light" | "dark" | null = null;

function theme(): "light" | "dark" {
    return document.documentElement.dataset.theme === "light" ? "light" : "dark";
}

function load(): Promise<Mermaid | null> {
    if (engine) {
        return Promise.resolve(engine);
    }
    loading ??= import("mermaid")
        .then((module) => {
            engine = module.default;
            return engine;
        })
        .catch((error) => {
            console.warn("mermaid 没装上来，图按原文显示：", error);
            return null;
        });
    return loading;
}

/** 把元素还原成"还没画"的样子（原文放回去，擦掉上次画的痕迹） */
function reset(element: HTMLElement): void {
    if (element.dataset.processed !== "true" && !element.dataset.problem) {
        return;
    }
    element.textContent = element.dataset.source ?? "";
    delete element.dataset.processed;
    delete element.dataset.problem;
    element.removeAttribute("title");
}

async function render(root: HTMLElement): Promise<void> {
    const nodes = [...root.querySelectorAll<HTMLElement>("pre.mermaid")].filter(
        (node) => node.isConnected,
    );
    if (nodes.length === 0) {
        return;
    }

    const mermaid = await load();
    if (!mermaid) {
        return;
    }

    const wanted = theme();
    if (wanted !== drawn) {
        for (const node of nodes) {
            reset(node);
        }
        mermaid.initialize({
            startOnLoad: false,
            theme: wanted === "light" ? "default" : "dark",
            // 图是笔记内容，不是代码：里面的 HTML 一律按文本处理
            securityLevel: "strict",
            // 出错时别自己往正文里塞一块红字，交给我们（原文留着更清楚）
            suppressErrorRendering: true,
            fontFamily: "inherit",
        });
        drawn = wanted;
    }

    for (const node of nodes) {
        if (node.dataset.processed === "true") {
            continue;
        }
        try {
            await mermaid.run({ nodes: [node] });
        } catch (error) {
            // 原文还在：mermaid 画不动的时候不会动元素内容
            node.dataset.problem = "画不出来";
            node.title = `这张图没能画出来：${error}`;
            console.warn("mermaid 画不出来：", error);
        }
    }
}

/** 攒多久：够把一串连着的按键并成一次，又短到不像卡住 */
const DIAGRAM_DELAY = 150;
let timer: number | undefined;
let pending: HTMLElement | null = null;

/** 排队的活儿：同一时刻只画一趟（mermaid 内部有全局状态，别并发叫它） */
let running: Promise<void> = Promise.resolve();

function queue(root: HTMLElement): void {
    running = running
        .then(() => render(root))
        .catch((error) => console.warn("画图这趟没走完：", error));
}

/**
 * 攒一下再画（编辑时每次按键都会重新注入 HTML）。
 *
 * 只有最后一次注入的那个 root 会被画：前几次的 DOM 早被换掉了。
 */
export function scheduleDiagrams(root: HTMLElement): void {
    pending = root;
    window.clearTimeout(timer);
    timer = window.setTimeout(() => {
        const target = pending;
        pending = null;
        if (target && target.isConnected) {
            queue(target);
        }
    }, DIAGRAM_DELAY);
}

/**
 * 切主题：mermaid 的配色是画的时候定死的，得把文档里已经画过的整体重画。
 *
 * 盯的是 `<html data-theme>` —— 那是 `core/theme.ts::applyTheme` 落笔的地方。
 * 重画统一走上面那条排队，别和正在画的撞上。
 */
new MutationObserver(() => {
    if (theme() === drawn) {
        return;
    }
    scheduleDiagrams(document.body);
}).observe(document.documentElement, {
    attributes: true,
    attributeFilter: ["data-theme"],
});
