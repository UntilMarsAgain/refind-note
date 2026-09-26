/**
 * 代码块的行号。
 *
 * 行号**不写进代码文本**，而是单独一列：写进去就会参与复制，也会被 highlight.js
 * 当成代码一起分词。横向滚动时那一列用 sticky 留在原地。
 */
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

    const gutter = document.createElement("div");
    gutter.className = "code-lines";
    // 行号只是给眼睛的参照，读屏时逐行念数字只会添乱
    gutter.setAttribute("aria-hidden", "true");
    for (let line = 1; line <= lineCount(code.textContent ?? ""); line += 1) {
      const item = document.createElement("span");
      item.textContent = String(line);
      gutter.append(item);
    }
    pre.prepend(gutter);
    pre.classList.add("pre--numbered");
  }
}
