/// <reference types="node" />
import assert from "node:assert/strict";
import { test } from "node:test";
import { markedLines, parseLineRanges } from "./code-marks.ts";

test("单个行号与区间", () => {
  assert.deepEqual(parseLineRanges("3"), [{ from: 3, to: 3 }]);
  assert.deepEqual(parseLineRanges("5-7"), [{ from: 5, to: 7 }]);
  assert.deepEqual(parseLineRanges("3,5-7"), [
    { from: 3, to: 3 },
    { from: 5, to: 7 },
  ]);
});

test("开区间：4- 表示到最后一行", () => {
  assert.deepEqual(parseLineRanges("4-"), [{ from: 4, to: null }]);
});

test("乱写的地方跳过，不牵连别的", () => {
  assert.deepEqual(parseLineRanges("3, 呵呵, 0, 7-5, -2"), [{ from: 3, to: 3 }]);
  assert.deepEqual(parseLineRanges(""), []);
  assert.deepEqual(parseLineRanges("  ,  "), []);
});

test("展开成行号：升序、去重、不超过总行数", () => {
  assert.deepEqual(markedLines("3,2,2", 10), [2, 3]);
  assert.deepEqual(markedLines("8-", 10), [8, 9, 10]);
  assert.deepEqual(markedLines("1-99", 3), [1, 2, 3]);
  assert.deepEqual(markedLines("5-9", 3), []);
});
