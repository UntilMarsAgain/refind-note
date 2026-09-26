/// <reference types="node" />
//! 模板块边界规则的用例。
//!
//! 用 Node 自带的测试跑（无需额外依赖）：`pnpm test:ts`
//!
//! **与后端的测试一一对应**（`src-tauri/src/markdown/syntax/template/mod.rs`）——
//! 这是防止"编辑器与渲染各说各话"的机制：规则改坏任何一边，两边都会当场报出来。
import assert from "node:assert/strict";
import { test } from "node:test";
import { headIndent, isTemplateHead, templateBlockEnd } from "./template-blocks.ts";

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
