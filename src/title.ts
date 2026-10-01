/**
 * 标题的字面处理。
 *
 * 只有一件事：列表里那个单字缩略图标。
 *
 * 标题的**合法性判定不在这里** —— 那属于地址解析（在后端）；
 * 前端复制一份就会多出一个真相来源。
 */

/**
 * 列表里那个单字图标：取**名称段**的首字。
 *
 * 不能取整个名字的首字：带命名空间前缀的标题会全变成前缀的那个字母 ——
 * 一堆一模一样的图标，等于没有图标（`special:debug` 该给 "d"，不是 "s"）。
 *
 * 用 `Array.from` 取，免得多字节字符或代理对被切成半个。
 */
export function initialOf(title: string): string {
  const trimmed = title.trim();
  // 名称段 = 第一个冒号之后的部分；没有冒号就是整个标题
  const separator = trimmed.indexOf(":");
  const name = (separator >= 0 ? trimmed.slice(separator + 1) : trimmed).trim();
  return Array.from(name || trimmed)[0] ?? "•";
}
