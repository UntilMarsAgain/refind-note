/**
 * 代码块的行号。
 *
 * 行号**不写进代码文本**，而是单独一列：写进去就会参与复制，也会被 highlight.js
 * 当成代码一起分词。横向滚动时那一列用 sticky 留在原地。
 */
import { markedLines } from "./code-marks";
import { codeLineNumbers } from "./settings";

/** 代码有多少行。末尾那个换行不算多出来的一行，空代码块算一行。 */
function lineCount(text: string): number {
  const trimmed = text.endsWith("\n") ? text.slice(0, -1) : text;
  return trimmed ? trimmed.split("\n").length : 1;
}

/**
 * 按当前偏好给根节点里的每个代码块加行号；关掉时把已有的行号去掉。
 *
 * 可以反复调用（v-html 重渲染、开关来回切都会走到这里）：每次先把上一轮加的
 * 行号列清掉，所以不会越叠越多。
 */
export function applyLineNumbers(
  root: HTMLElement,
  on: boolean = codeLineNumbers.value,
): void {
  for (const pre of root.querySelectorAll("pre")) {
    for (const child of Array.from(pre.children)) {
      if (child.classList.contains("code-lines")) {
        child.remove();
      }
    }
    pre.classList.remove("pre--numbered");

    if (!on) {
      continue;
    }

    const code = pre.querySelector("code");
    if (!code) {
      continue;
    }
    const total = lineCount(code.textContent ?? "");

    // 单块可以自己说了算：`::code lines=off` 就是不显示行号
    if (pre.dataset.lines === "off") {
      continue;
    }

    // 起始号：`::code start=10` 从 10 开始数（写文档时常常要接着上文编号）
    const start = Number.parseInt(pre.dataset.lineStart ?? "1", 10);
    const first = Number.isFinite(start) ? start : 1;

    const gutter = document.createElement("div");
    gutter.className = "code-lines";
    // 行号只是给眼睛的参照，读屏时逐行念数字只会添乱
    gutter.setAttribute("aria-hidden", "true");
    for (let line = 0; line < total; line += 1) {
      const item = document.createElement("span");
      item.textContent = String(first + line);
      gutter.append(item);
    }
    pre.prepend(gutter);
    pre.classList.add("pre--numbered");

    paintMarkedLines(pre, total);
  }
}

/**
 * 把要强调的行画上底色。
 *
 * 用一层**绝对定位的色带**盖在代码上，而不是把代码拆成一行一个元素：
 * 拆行会破坏 highlight.js 已经生成的分词（它的 span 可能跨行），得不偿失。
 * 色带只按行高定位，不参与布局，也不挡选中（`pointer-events: none`）。
 */
function paintMarkedLines(pre: HTMLElement, total: number) {
  const spec = pre.dataset.highlight ?? "";
  const lines = markedLines(spec, total);
  if (lines.length === 0) {
    return;
  }

  const preStyle = getComputedStyle(pre);
  // 行高与内边距都从**真正排版文字的那一层**（code）取：它可能与 pre 各有各的内边距
  const code = pre.querySelector("code") ?? pre;
  const codeStyle = getComputedStyle(code);
  const lineHeight = Number.parseFloat(codeStyle.lineHeight);
  if (!Number.isFinite(lineHeight) || lineHeight <= 0) {
    return;
  }

  /*
   * 色带的起点：**实测**代码内容区的上沿，而不是假设"只有 pre 有内边距"。
   *
   * 这里踩过一次：`pre` 与它里面的 `code` 各有一份内边距，只减一份就会整体偏上十几像素 ——
   * 行号对得上、色带却压在文字上头。实测两个盒子的位置差，就不必关心谁有几个内边距。
   *
   * 两处都随横向滚动一起移动，所以差值不受滚动影响。
   */
  const preRect = pre.getBoundingClientRect();
  const codeRect = code.getBoundingClientRect();
  const offset =
    codeRect.top -
    preRect.top -
    (Number.parseFloat(preStyle.borderTopWidth) || 0) +
    (Number.parseFloat(codeStyle.paddingTop) || 0);

  const marks = document.createElement("div");
  marks.className = "code-marks";
  marks.setAttribute("aria-hidden", "true");
  for (const line of lines) {
    const band = document.createElement("span");
    band.className = "code-mark";
    band.style.top = offset + (line - 1) * lineHeight + "px";
    band.style.height = lineHeight + "px";
    marks.append(band);
  }
  pre.prepend(marks);
  pre.classList.add("pre--marked");
}
