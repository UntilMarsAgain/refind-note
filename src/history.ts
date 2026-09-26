//! 浏览历史：记录看过的页面。
//!
//! 存在 localStorage，因为它是**界面状态**而不是仓库数据 —— 就像浏览器历史不属于某个网站。
//! 于是它在设置里可以单独关掉，清空也只清它一个。
//!
//! 关掉之后**不再记录**，但已有记录留着：让用户自己决定什么时候清空，
//! 而不是"一关就把你之前的东西删掉"。
import { ref } from "vue";
import { PREFERENCE_KEYS, readFlag, readPreference, writeFlag, writePreference } from "./settings";
import { type HistoryEntry, keepValid, withVisit } from "./history-list";

// 列表逻辑在 history-list.ts（纯函数、可测），这里只管存储与响应式状态
export { HISTORY_LIMIT, type HistoryEntry, withVisit } from "./history-list";

function read(): HistoryEntry[] {
  const raw = readPreference(PREFERENCE_KEYS.history);
  if (!raw) {
    return [];
  }
  try {
    return keepValid(JSON.parse(raw));
  } catch {
    return [];
  }
}

export const historyEnabled = ref(readFlag(PREFERENCE_KEYS.historyOn, true));

export const browsingHistory = ref<HistoryEntry[]>(read());

export function setHistoryEnabled(on: boolean): void {
  historyEnabled.value = on;
  writeFlag(PREFERENCE_KEYS.historyOn, on);
}

/** 记一次浏览。关掉时什么都不做（连存储也不动）。 */
export function recordVisit(address: string, title: string): void {
  if (!historyEnabled.value || !address) {
    return;
  }
  const next = withVisit(browsingHistory.value, {
    address,
    title: title || address,
    at: new Date().toISOString(),
  });
  browsingHistory.value = next;
  writePreference(PREFERENCE_KEYS.history, JSON.stringify(next));
}

export function clearHistory(): void {
  browsingHistory.value = [];
  writePreference(PREFERENCE_KEYS.history, "[]");
}
