//! 模板块的边界规则 —— 与后端逐条一致的那一份。
//!
//! 后端扫描器（`src-tauri/src/markdown/syntax/template/mod.rs`）是规则的**定义处**，
//! 这里是它的镜像：编辑器要在打字时画出块的边界，不可能每按一键就问一次后端。
//! 两边必须同规，所以规则抽成一个**不依赖 CM6 的纯函数**，并配一份用例
//! （`template-blocks.test.ts`）——用例与后端的测试一一对应，改坏哪边都会当场发现。
//!
//! 三条规则：
//!
//! 1. 头行之后，缩进更深（非空）的行属于块内；
//! 2. 遇到缩进不更深的行，块到此为止；
//! 3. 空行**不直接结束块** —— 往后看一行：后面还有更深的内容，这个空行才算块内。

/** 一行开头的空白字符数（只数空格与制表符，与后端同一口径） */
export function headIndent(text: string): number {
  return text.length - text.trimStart().length;
}

/** 是不是空行（只有空白） */
export function isBlank(text: string): boolean {
  return text.trim().length === 0;
}

/**
 * 是不是模板块的头行：`::名字`。
 *
 * 与后端 `parse_header` 同一口径：名字是**第一个 token**，且其中不许出现 `=` 或 `:`；
 * 引号包住的名字（可以有空格）也算一个 token。
 *
 * 注意不能只用正则匹配前缀：`::a:b` 的前缀 `a` 看着像名字，但整个 token 是 `a:b` ——
 * 后端会把它退回普通段落，编辑器也必须这样认（否则高亮又是一处"两边各说各话"）。
 */
export function isTemplateHead(text: string): boolean {
  const trimmed = text.trimStart();
  if (!trimmed.startsWith("::")) {
    return false;
  }
  const rest = trimmed.slice(2);
  if (rest.length === 0) {
    return false;
  }
  const first = rest[0];
  if (first === '"' || first === "'") {
    // 引号名：要有配对的引号（与后端的词法一致：引号没关上就不算头）
    return rest.indexOf(first, 1) > 0;
  }
  const token = rest.split(/\s/)[0] ?? "";
  return token.length > 0 && !token.includes("=") && !token.includes(":");
}

/**
 * 块的最后一行（0 基下标，含端点）；一条都不属于块内时返回 `start`。
 *
 * `lines` 是全文按行切开的数组，`start` 是头行的下标，`markerIndent` 是头行的缩进。
 */
export function templateBlockEnd(
  lines: string[],
  start: number,
  markerIndent: number,
): number {
  let last = start;
  for (let number = start + 1; number < lines.length; number += 1) {
    const text = lines[number] ?? "";
    if (isBlank(text)) {
      // 往后找第一个非空行：它更深就把这个空行收进来，否则到此为止
      let next = number + 1;
      while (next < lines.length && isBlank(lines[next] ?? "")) {
        next += 1;
      }
      if (next >= lines.length || headIndent(lines[next] ?? "") <= markerIndent) {
        break;
      }
      last = number;
      continue;
    }
    if (headIndent(text) <= markerIndent) {
      break;
    }
    last = number;
  }
  return last;
}
