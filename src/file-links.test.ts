/// <reference types="node" />
import assert from "node:assert/strict";
import { test } from "node:test";
import {
  fileNameOfUrl,
  fileReferenceOf,
  fileTargetOf,
  httpUrlOf,
  vaultKeyOf,
} from "./file-links.ts";

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

test("引用写法：图片写图，其它写链接", () => {
  assert.equal(fileReferenceOf({ name: "桥.png", mime: "image/png" }), "![桥.png](桥.png)");
  assert.equal(fileReferenceOf({ name: "说明.pdf", mime: "application/pdf" }), "[说明.pdf](说明.pdf)");
});

test("从取件地址反解出文件名；别的地址一律不算", () => {
  assert.equal(vaultKeyOf("refind://localhost/files/%E6%A1%A5.png"), "桥.png");
  assert.equal(vaultKeyOf("refind://localhost/files/a.png"), "a.png");
  // 外部图片拿不到字节，所以不是"仓库里的文件"
  assert.equal(vaultKeyOf("https://example.com/a.png"), null);
  assert.equal(vaultKeyOf("/logo.svg"), null);
  assert.equal(vaultKeyOf("refind://localhost/files/"), null);
  // 坏编码给 null，不抛
  assert.equal(vaultKeyOf("refind://localhost/files/%ZZ"), null);
});

test("只有 http/https 才算可下载的网址", () => {
  assert.equal(httpUrlOf("https://example.com/a.png"), "https://example.com/a.png");
  assert.equal(httpUrlOf(" http://example.com/a.png "), "http://example.com/a.png");
  // 这些都不该从"另存网页图片"那条路上走
  assert.equal(httpUrlOf("file:///etc/passwd"), null);
  assert.equal(httpUrlOf("data:image/png;base64,AAAA"), null);
  assert.equal(httpUrlOf("/logo.svg"), null);
  assert.equal(httpUrlOf("refind://localhost/files/a.png"), null);
});

test("另存网页图片的默认名从网址猜", () => {
  assert.equal(fileNameOfUrl("https://example.com/a/b.png?y=1"), "b.png");
  assert.equal(fileNameOfUrl("https://example.com/a/%E6%A1%A5.png"), "桥.png");
  // 猜不出就给一个看着像话的名字，不猜路径
  assert.equal(fileNameOfUrl("https://example.com/"), "图片");
  assert.equal(fileNameOfUrl("https://example.com"), "example.com");
  // 坏编码原样用，不抛
  assert.equal(fileNameOfUrl("https://example.com/%ZZ"), "%ZZ");
});
