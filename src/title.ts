/**
 * 标题的**即时词法检查**。
 *
 * ⚠️ 这里只是手感：真正的权威判定在后端（`validate_title` / 保存时的 `parse`）。
 * 之所以不把规则整份搬过来，是因为「冒号前缀是不是已知命名空间」依赖命名空间表，
 * 而表在仓库里（namespaces.json）—— 复制一份到前端就会出现第二个真相来源，改一处
 * 忘一处必然对不上。
 *
 * 后端的两条本项目规则：标题里不允许 `@`（地址栏用「标题@版本」），也不允许 `:`
 * （目前只有主命名空间，任何冒号前缀都判非法）。
 */

/** MediaWiki 的非法字符，外加本项目自己的 `:` 与 `@` */
const ILLEGAL = ["#", "<", ">", "[", "]", "|", "{", "}", ":", "@", "$"];

/** 与后端 `max_title_bytes` 对应（后端权威） */
const MAX_TITLE_BYTES = 255;

const encoder = new TextEncoder();

/** 返回人话的拒绝理由；`null` 表示词法上看着没问题（仍要由后端定夺） */
export function checkTitle(title: string): string | null {
  const trimmed = title.trim();
  if (!trimmed) {
    return "标题不能为空";
  }
  for (const ch of ILLEGAL) {
    if (trimmed.includes(ch)) {
      if (ch === "@") {
        return "标题里不能有 @（地址栏用「标题@版本」表达版本）";
      }
      if (ch === "$") {
        return "标题里不能有 $（地址栏用「标题$edit」「标题$history」表达状态）";
      }
      if (ch === "#") {
        return "标题里不能有 #（地址栏用「名称#章节」表达章节）";
      }
      return `标题里不能有 ${ch}`;
    }
  }
  if (encoder.encode(trimmed).length > MAX_TITLE_BYTES) {
    return `标题过长（上限 ${MAX_TITLE_BYTES} 字节）`;
  }
  return null;
}

/** 把地址栏那一行拆成「标题」与「版本」；没写 @ 版本时版本是 null */
export function splitTitleAndVersion(input: string): {
  title: string;
  version: number | null;
} {
  const raw = input.trim();
  const at = raw.lastIndexOf("@");
  if (at > 0) {
    const version = Number(raw.slice(at + 1));
    if (Number.isInteger(version) && version > 0) {
      return { title: raw.slice(0, at).trim(), version };
    }
  }
  return { title: raw, version: null };
}
