/// <reference types="node" />
//! 模板块边界规则的用例。
//!
//! 用 Node 自带的测试跑（无需额外依赖）：`pnpm test:ts`
//!
//! **与后端的测试一一对应**（`src-tauri/src/markdown/syntax/template/mod.rs`）——
//! 这是防止"编辑器与渲染各说各话"的机制：规则改坏任何一边，两边都会当场报出来。
import assert from "node:assert/strict";
import { test } from "node:test";
import {
  headIndent,
  isTemplateHead,
  templateBlockEnd,
  templateMarks,
  templateBlockLines,
  templateFoldRange,
  templateRanges,
} from "./template-blocks.ts";

/** 便捷：算"块覆盖了哪些行"（1 基，含端点），便于与后端用例对照 */
function covered(text: string, start = 1): string[] {
  const lines = text.split("\n");
  const index = start - 1;
  const last = templateBlockEnd(lines, index, headIndent(lines[index] ?? ""));
  return lines.slice(index, last + 1);
}

test("更深的行属于块内", () => {
  assert.deepEqual(covered("::note\n  块内\n块外\n"), ["::note", "  块内"]);
});

test("空行不直接结束块：后面还有更深的内容就算块内", () => {
  // 这正是"高亮比渲染早收"的那个情形
  assert.deepEqual(covered("::quote\n  第一段\n\n  第二段\n块外\n"), [
    "::quote",
    "  第一段",
    "",
    "  第二段",
  ]);
});

test("空行后面是不更深的内容：空行属于块外", () => {
  assert.deepEqual(covered("::note\n  块内\n\n块外\n"), ["::note", "  块内"]);
});

test("连续多个空行也一起跳过再判断", () => {
  assert.deepEqual(covered("::note\n  一\n\n\n  二\n"), ["::note", "  一", "", "", "  二"]);
});

test("嵌套的模板块在父块的范围内", () => {
  const lines = covered("::quote\n  ::quote origin=\"内层\"\n    文字\n");
  assert.deepEqual(lines, ["::quote", '  ::quote origin="内层"', "    文字"]);
});

test("更深层的相对结构不影响边界", () => {
  assert.deepEqual(covered("::note\n    四空格\n      六空格\n"), [
    "::note",
    "    四空格",
    "      六空格",
  ]);
});

test("空行在文末：不算块内", () => {
  assert.deepEqual(covered("::note\n  内容\n\n"), ["::note", "  内容"]);
});

test("认得出头行：引号包住的名字也算", () => {
  assert.equal(isTemplateHead("::quote origin=标题"), true);
  assert.equal(isTemplateHead('::"两 个词"'), true);
  assert.equal(isTemplateHead("  ::盒子"), true);
  // 与后端同一条规矩：名字是第一个 token，且其中不该有 `=` / `:`
  assert.equal(isTemplateHead("::=x"), false);
  assert.equal(isTemplateHead("::a:b"), false);
  assert.equal(isTemplateHead("::a=b"), false);
  assert.equal(isTemplateHead("::"), false);
  assert.equal(isTemplateHead("不是头行"), false);
  // 引号没关上也不算（与后端词法一致）
  assert.equal(isTemplateHead('::"没关上'), false);
});

test("块内的行只标缩进，不染到正文", () => {
  const marks = templateMarks(["::quote", "  正文一行", "    更深的正文"]);
  assert.deepEqual(marks, [
    { line: 0, head: true, start: 0, end: 7 },
    { line: 1, head: false, start: 0, end: 2 },
    { line: 2, head: false, start: 0, end: 4 },
  ]);
});

test("嵌套的头行只算头行，不与外层的块内叠加", () => {
  // 长度从字符串本身算：手写数字正是最容易抄错的东西
  const inner = '  ::quote origin="内层"';
  const marks = templateMarks([
    "::quote",
    "  外层正文",
    "",
    inner,
    "    内层正文",
  ]);
  assert.deepEqual(marks, [
    { line: 0, head: true, start: 0, end: 7 },
    { line: 1, head: false, start: 0, end: 2 },
    { line: 3, head: true, start: 0, end: inner.length },
    { line: 4, head: false, start: 0, end: 4 },
  ]);
});

test("空行不产生装饰（零长度标记会被 CM6 拒绝）", () => {
  const marks = templateMarks(["::note", "  一", "", "  二"]);
  assert.equal(marks.every((mark) => mark.end > mark.start), true);
  assert.deepEqual(
    marks.map((mark) => mark.line),
    [0, 1, 3],
  );
});

test("带空白的空行也不标（否则会留下一条孤零零的色块）", () => {
  // 打字时很容易在空行上留下空格，而它仍然是空行
  const marks = templateMarks(["::note", "  一", "    ", "  二"]);
  assert.deepEqual(
    marks.map((mark) => mark.line),
    [0, 1, 3],
  );
});

test("偏移量与逐字符累加一致，且不含空标记", () => {
  const lines = ["::quote", "  正文", "", "  再一段"];
  // 期望值由"逐字符累加"算出来，**不手写数字** —— 手写数字正是最容易抄错的东西
  // （这条用例第一版就抄错了一个：14 写成了 19）。
  const start = (index: number) =>
    lines.slice(0, index).reduce((sum, text) => sum + text.length + 1, 0);
  assert.deepEqual(templateRanges(lines), [
    { from: start(0), to: start(0) + 7, head: true },
    // 空行（第 2 行）不产生任何范围
    { from: start(1), to: start(1) + 2, head: false },
    { from: start(3), to: start(3) + 2, head: false },
  ]);
});

test("块覆盖的行包含块内的空行（这样左边缘才跨得过去）", () => {
  const lines = ["::quote", "  第一段", "  ", "", "  第二段", "块外"];
  // 第 2 行是"带空白的空行"、第 3 行是真正的空行：两者都属于块内
  assert.deepEqual(templateBlockLines(lines), [0, 1, 2, 3, 4]);
});

test("块外的行不覆盖", () => {
  assert.deepEqual(templateBlockLines(["::note", "  块内", "", "块外"]), [0, 1]);
});

test("折叠范围跨过块内空行，一直到块尾", () => {
  const lines = ["::quote", "  第一段", "", "  第二段", "块外"];
  assert.deepEqual(templateFoldRange(lines, 0), { from: 1, to: 3 });
});

test("没有内容的头行不折叠（不该出现折叠箭头）", () => {
  assert.equal(templateFoldRange(["::note", "块外"], 0), null);
});

test("不是头行就没有折叠范围", () => {
  assert.equal(templateFoldRange(["普通一行", "  缩进"], 0), null);
});
