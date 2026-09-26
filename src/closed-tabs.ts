//! 刚关掉的标签页（Ctrl+Shift+T 用）。
//!
//! 与浏览器一致：**后进先出**，而且只留最近若干个 —— 一路关下去的标签页不该永远占着内存，
//! 而"关错了想找回来"要用的也只是最近那几个。
//!
//! 纯函数、对元素类型没有要求，所以可以直接测：上限该丢哪一头、取的时候拿哪个，
//! 这两件事想错了都不容易当场发现。
/** 最多记住多少个关掉的标签页 */
export const CLOSED_LIMIT = 12;

/** 压入一个刚关掉的标签页（最新的在最前，超出上限时丢**最老的**） */
export function pushClosed<T>(stack: T[], closed: T, limit: number = CLOSED_LIMIT): T[] {
  return [closed, ...stack].slice(0, limit);
}

/** 取出最近关掉的那个；一个都没有时返回 `null` */
export function popClosed<T>(stack: T[]): { closed: T; rest: T[] } | null {
  if (stack.length === 0) {
    return null;
  }
  const [closed, ...rest] = stack;
  return { closed: closed as T, rest };
}
