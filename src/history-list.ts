//! 浏览历史的列表逻辑。
//!
//! 单独一个模块、不引入任何依赖：存储那侧要碰 localStorage，而"同一页连着看两次算几条"
//! 这种事想错了不容易发现、定下来又是一句话 —— 所以它值得被单独测。
//! 存储与响应式状态在 `history.ts`。

/** 最多留多少条。定上限是因为 localStorage 不是无限大的，而历史越老越没用。 */
export const HISTORY_LIMIT = 300;

export interface HistoryEntry {
  /** 地址栏里那串字（点它就能回到那一页） */
  address: string;
  /** 当时那一页的标题（可能随后被改名，这里记的是当时看到的） */
  title: string;
  /** 打开的时间（ISO 字符串） */
  at: string;
}

/**
 * 把一次浏览并进列表：最新的在前、同一页只留最近一次、并裁到上限。
 */
export function withVisit(
  entries: HistoryEntry[],
  visit: HistoryEntry,
  limit: number = HISTORY_LIMIT,
): HistoryEntry[] {
  const rest = entries.filter((entry) => entry.address !== visit.address);
  return [visit, ...rest].slice(0, limit);
}

/**
 * 只留形状对的条目：存储里的东西可能是旧版本写的，读到坏的不该让整页打不开。
 */
export function keepValid(parsed: unknown): HistoryEntry[] {
  if (!Array.isArray(parsed)) {
    return [];
  }
  return parsed.filter(
    (item): item is HistoryEntry =>
      typeof item === "object" &&
      item !== null &&
      typeof (item as HistoryEntry).address === "string" &&
      typeof (item as HistoryEntry).title === "string" &&
      typeof (item as HistoryEntry).at === "string",
  );
}
