/// <reference types="node" />
import assert from "node:assert/strict";
import { test } from "node:test";
import { fileTargetOf } from "./note-html.ts";

test("相对地址当作仓库里的文件", () => {
  assert.equal(fileTargetOf("图片.png"), "图片.png");
  assert.equal(fileTargetOf("  桥 图.png  "), "桥 图.png");
  assert.equal(fileTargetOf("./图/桥.png"), "./图/桥.png");
});

test("绝对地址与程序自己的资源一律不动", () => {
  // 带冒号的一律当绝对地址：http、https、data、asset 都在内
  for (const source of [
    "http://example.com/a.png",
    "https://example.com/a.png",
    "data:image/png;base64,AAAA",
    "asset://localhost/x.png",
  ]) {
    assert.equal(fileTargetOf(source), null, source);
  }
  // 以 `/` 开头的是程序自己的静态资源（示例里的 /logo.svg 就是）
  assert.equal(fileTargetOf("/logo.svg"), null);
  assert.equal(fileTargetOf(""), null);
  assert.equal(fileTargetOf("   "), null);
  assert.equal(fileTargetOf("#anchor"), null);
});
