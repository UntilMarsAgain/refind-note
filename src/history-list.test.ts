/// <reference types="node" />
import assert from "node:assert/strict";
import { test } from "node:test";
import { HISTORY_LIMIT, type HistoryEntry, keepValid, withVisit } from "./history-list.ts";

function visit(address: string, at: string) {
  return { address, title: address, at };
}

test("最新的排在最前", () => {
  const first = withVisit([], visit("甲", "1"));
  const second = withVisit(first, visit("乙", "2"));
  assert.deepEqual(
    second.map((entry) => entry.address),
    ["乙", "甲"],
  );
});

test("同一页再看一次只留最近那一条（不堆重复）", () => {
  let list = withVisit([], visit("甲", "1"));
  list = withVisit(list, visit("乙", "2"));
  list = withVisit(list, visit("甲", "3"));
  assert.deepEqual(
    list.map((entry) => [entry.address, entry.at]),
    [
      ["甲", "3"],
      ["乙", "2"],
    ],
  );
});

test("超过上限就裁掉最老的", () => {
  // 显式标注：测试里也从空数组开始累积，不写类型会被推断成 any[]
  let list: HistoryEntry[] = [];
  for (let index = 0; index < HISTORY_LIMIT + 20; index += 1) {
    list = withVisit(list, visit("页" + index, String(index)));
  }
  assert.equal(list.length, HISTORY_LIMIT);
  // 最新的还在，最老的没了
  assert.equal(list[0]?.address, "页" + (HISTORY_LIMIT + 19));
  assert.equal(
    list.some((entry) => entry.address === "页0"),
    false,
  );
});

test("读到坏数据只丢坏的那几条，不整页打不开", () => {
  assert.deepEqual(keepValid("不是数组"), []);
  assert.deepEqual(keepValid([{ address: "甲" }]), []);
  const good = { address: "甲", title: "甲", at: "1" };
  assert.deepEqual(keepValid([good, { address: 1 }, null, "字"]), [good]);
});
