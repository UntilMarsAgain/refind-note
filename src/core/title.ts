/**
 * 标题的字面处理：列表缩略图标，以及子页面父子关系。
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

/**
 * 子页面的上一级：按**最后一个**斜杠切，只上去一层。
 *
 * `Test/航道/水文` 的上一级是 `Test/航道`，不是 `Test` —— 一层一层退才走得清楚。
 * 开头的斜杠（`/航道`）不算父级：那是"当前页的子页"，没有独立的名字可回。
 */
export function parentOf(title: string): string {
  const cut = title.lastIndexOf("/");
  return cut > 0 ? title.slice(0, cut) : "";
}
