/// <reference types="node" />
import assert from "node:assert/strict";
import { test } from "node:test";
import { CLOSED_LIMIT, popClosed, pushClosed } from "./closed-tabs.ts";

test("后进先出：最后关掉的最先回来", () => {
  let stack: string[] = [];
  stack = pushClosed(stack, "甲");
  stack = pushClosed(stack, "乙");
  const first = popClosed(stack);
  assert.equal(first?.closed, "乙");
  const second = popClosed(first?.rest ?? []);
  assert.equal(second?.closed, "甲");
  assert.deepEqual(second?.rest, []);
});

test("空栈取出是 null，不是抛错", () => {
  assert.equal(popClosed([]), null);
});

test("超过上限时丢的是最老的，不是最新的", () => {
  let stack: number[] = [];
  for (let index = 0; index < CLOSED_LIMIT + 5; index += 1) {
    stack = pushClosed(stack, index);
  }
  assert.equal(stack.length, CLOSED_LIMIT);
  // 最新那个还在最前
  assert.equal(stack[0], CLOSED_LIMIT + 4);
  // 最老的（0..4）已经被丢掉
  assert.equal(stack.includes(0), false);
});
