/// <reference types="node" />
import assert from "node:assert/strict";
import { test } from "node:test";
import { pastedNames } from "./paste-files.ts";

const at = new Date(2026, 8, 26, 15, 4, 5);

test("有真名字就用真名字", () => {
  assert.deepEqual(pastedNames([{ name: "桥体.png", type: "image/png" }], at), ["桥体.png"]);
});

test("截图没有真名字时按类型起名", () => {
  // 浏览器给粘贴的图片一个占位名，等于没名字
  for (const placeholder of ["image.png", "blob", ""]) {
    assert.deepEqual(
      pastedNames([{ name: placeholder, type: "image/png" }], at),
      ["粘贴-20260926-150405.png"],
    );
  }
});

test("认不出的类型给 bin，不猜", () => {
  assert.deepEqual(pastedNames([{ name: "", type: "application/x-unknown" }], at), [
    "粘贴-20260926-150405.bin",
  ]);
});

test("一次粘好几个时互不撞名", () => {
  const files = [
    { name: "", type: "image/png" },
    { name: "", type: "image/jpeg" },
  ];
  assert.deepEqual(pastedNames(files, at), [
    "粘贴-20260926-150405-1.png",
    "粘贴-20260926-150405-2.jpg",
  ]);
});
