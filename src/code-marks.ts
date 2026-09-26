//! 代码块要标出哪几行。
//!
//! 纯函数，单独一个文件：解析这种"人和机器都要看"的小语法最容易出岔子
//! （`3,5-7`、`4-`、乱写），而它又完全不需要 DOM，所以可以直接测。
//!
//! 写法：逗号分隔的**行号或区间**，区间用 `-`；`4-` 表示"第 4 行到最后"。
//! 行号从 1 起（人读的号），不随 `start` 参数偏移 —— 作者写的是**屏幕上看到的号**。

/** 一段区间（含两端，1 起） */
export interface LineRange {
  from: number;
  to: number | null; // null = 到最后一行
}

/**
 * 解析 `highlight` 参数。乱写的地方**跳过**而不是整条作废：
 * 一个笔误不该让整块代码失去高亮，能标出来的先标出来。
 */
export function parseLineRanges(text: string): LineRange[] {
  const ranges: LineRange[] = [];
  for (const piece of text.split(",")) {
    const token = piece.trim();
    if (!token) {
      continue;
    }
    const dash = token.indexOf("-");
    if (dash < 0) {
      const line = Number.parseInt(token, 10);
      if (Number.isFinite(line) && line > 0) {
        ranges.push({ from: line, to: line });
      }
      continue;
    }
    const from = Number.parseInt(token.slice(0, dash).trim(), 10);
    const rest = token.slice(dash + 1).trim();
    if (!Number.isFinite(from) || from <= 0) {
      continue;
    }
    if (rest === "") {
      ranges.push({ from, to: null });
      continue;
    }
    const to = Number.parseInt(rest, 10);
    if (Number.isFinite(to) && to >= from) {
      ranges.push({ from, to });
    }
  }
  return ranges;
}

/** 展开成具体的行号（升序、去重），`total` 是这块代码的总行数 */
export function markedLines(text: string, total: number): number[] {
  const lines = new Set<number>();
  for (const range of parseLineRanges(text)) {
    const last = range.to ?? total;
    for (let line = range.from; line <= Math.min(last, total); line += 1) {
      lines.add(line);
    }
  }
  return [...lines].sort((a, b) => a - b);
}
