/// <reference types="node" />
//! 标题工具的用例。
//!
//! 与模板块那边一样用 Node 自带的测试跑（`pnpm test:ts`），不引入额外依赖。
import assert from "node:assert/strict";
import { test } from "node:test";
import { initialOf } from "./title.ts";

test("缩略字取名称段的首字，不是命名空间前缀", () => {
  // 这就是用户报的那个问题：模板命名空间里全是 "t"
  assert.equal(initialOf("template:样式.css"), "样");
  assert.equal(initialOf("template:盒子"), "盒");
  // 顺带：特殊页面也不再全是 "s"
  assert.equal(initialOf("special:debug"), "d");
  assert.equal(initialOf("help:入门"), "入");
});

test("主命名空间（没有前缀）取标题首字", () => {
  assert.equal(initialOf("平陆运河"), "平");
  assert.equal(initialOf("  A B  "), "A");
});

test("多字节与代理对不会被切成半个", () => {
  assert.equal(initialOf("template:🚀 火箭"), "🚀");
  assert.equal(initialOf("𠮷野家"), "𠮷");
});

test("空标题有兜底，不崩", () => {
  assert.equal(initialOf(""), "•");
  assert.equal(initialOf("   "), "•");
  // 名称段为空时退回整个标题，至少还有个字
  assert.equal(initialOf("template:"), "t");
});
