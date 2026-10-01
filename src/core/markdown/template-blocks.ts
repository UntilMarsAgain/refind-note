/**
 * 模板块的边界规则。
 *
 * 后端扫描器是规则的**定义处**，这里是它的镜像：渲染与高亮必须同规，
 * 所以规则抽成一个**不依赖具体编辑器的纯函数**，两边对不上时一眼能看出来。
 *
 * 三条规则：
 *
 * 1. 头行之后，缩进更深（非空）的行属于块内；
 * 2. 遇到缩进不更深的行，块到此为止；
 * 3. 空行**不直接结束块** —— 往后看一行：后面还有更深的内容，这个空行才算块内。
 */

/** 一行开头的空白字符数（只数空格与制表符，与后端同一口径） */
export function headIndent(text: string): number {
  return text.length - text.trimStart().length;
}

/** 是不是空行（只有空白） */
export function isBlank(text: string): boolean {
  return text.trim().length === 0;
}

/**
 * 是不是模板块的头行：`::名字`。
 *
 * 与后端 `parse_header` 同一口径：名字是**第一个 token**，且其中不许出现 `=` 或 `:`；
 * 引号包住的名字（可以有空格）也算一个 token。
 *
 * 注意不能只用正则匹配前缀：`::a:b` 的前缀 `a` 看着像名字，但整个 token 是 `a:b` ——
 * 后端会把它退回普通段落，这里也必须这样认（否则高亮又是一处"两边各说各话"）。
 */
export function isTemplateHead(text: string): boolean {
  const trimmed = text.trimStart();
  if (!trimmed.startsWith("::")) {
    return false;
  }
  const rest = trimmed.slice(2);
  if (rest.length === 0) {
    return false;
  }
  const first = rest[0];
  if (first === '"' || first === "'") {
    // 引号名：要有配对的引号（与后端的词法一致：引号没关上就不算头）
    return rest.indexOf(first, 1) > 0;
  }
  const token = rest.split(/\s/)[0] ?? "";
  return token.length > 0 && !token.includes("=") && !token.includes(":");
}

/**
 * 头行里的**名字**（`::js` → `js`）；不是头行就是空串。
 *
 * 与 [`isTemplateHead`] 同一套认法：引号名去掉引号，普通名字取第一个 token。
 */
export function templateNameOf(text: string): string {
  const trimmed = text.trimStart();
  if (!trimmed.startsWith("::")) {
    return "";
  }
  const rest = trimmed.slice(2);
  const first = rest[0];
  if (first === '"' || first === "'") {
    const end = rest.indexOf(first, 1);
    return end > 0 ? rest.slice(1, end) : "";
  }
  const token = rest.split(/\s/)[0] ?? "";
  return token.includes("=") || token.includes(":") ? "" : token;
}

/** 头行里的 `lang=`（引号可有可无），小写；没写就是空串 */
function languageOf(head: string): string {
  const found = /lang=(?:"([^"]*)"|'([^']*)'|(\S+))/.exec(head);
  return (found?.[1] ?? found?.[2] ?? found?.[3] ?? "").toLowerCase();
}

/** 一块要按 JavaScript 上色的内容：文档偏移量与正文 */
export interface ScriptRange {
  from: number;
  to: number;
  code: string;
}

/**
 * 编辑器该按 **JavaScript** 上色的那几块（偏移量是**块内**，不含头行）。
 *
 * 两类：`::js` 的正文（那个模板的内容本来就是 JS），以及 `::code lang=js` 的正文 ——
 * 后者是"当代码显示"，但显示成一片没有颜色的字也白搭。
 *
 * 规则仍在这一处：编辑器的高亮与后端"这段是什么"必须同规，否则高亮会说谎。
 */
export function scriptRanges(lines: string[]): ScriptRange[] {
  const starts: number[] = [];
  let offset = 0;
  for (const text of lines) {
    starts.push(offset);
    offset += text.length + 1; // 加上被 split 掉的那个换行
  }

  const out: ScriptRange[] = [];
  for (let index = 0; index < lines.length; index += 1) {
    const head = lines[index] ?? "";
    if (!isTemplateHead(head)) {
      continue;
    }
    const name = templateNameOf(head);
    const isScript =
      name === "js" ||
      (name === "code" && ["js", "javascript"].includes(languageOf(head)));
    if (!isScript) {
      continue;
    }

    const last = templateBlockEnd(lines, index, headIndent(head));
    if (last <= index) {
      continue;
    }
    const from = starts[index + 1] ?? 0;
    const to = (starts[last] ?? 0) + (lines[last] ?? "").length;
    out.push({ from, to, code: lines.slice(index + 1, last + 1).join("\n") });
  }
  return out;
}

/**
 * 块的最后一行（0 基下标，含端点）；一条都不属于块内时返回 `start`。
 *
 * `lines` 是全文按行切开的数组，`start` 是头行的下标，`markerIndent` 是头行的缩进。
 */
export function templateBlockEnd(
  lines: string[],
  start: number,
  markerIndent: number,
): number {
  let last = start;
  for (let number = start + 1; number < lines.length; number += 1) {
    const text = lines[number] ?? "";
    if (isBlank(text)) {
      // 往后找第一个非空行：它更深就把这个空行收进来，否则到此为止
      let next = number + 1;
      while (next < lines.length && isBlank(lines[next] ?? "")) {
        next += 1;
      }
      if (next >= lines.length || headIndent(lines[next] ?? "") <= markerIndent) {
        break;
      }
      last = number;
      continue;
    }
    if (headIndent(text) <= markerIndent) {
      break;
    }
    last = number;
  }
  return last;
}

/** 一条装饰：`line` 是 0 基行号，`start` / `end` 是**行内列**（0 基，end 不含） */
export interface TemplateMark {
  line: number;
  head: boolean;
  start: number;
  end: number;
}

/**
 * 把全文算成一串装饰（不含文档偏移量换算，所以可以单独验算）。
 *
 * - **头行**：整行 —— 它是一次模板调用，整行标出来才看得出；
 * - **块内的其他行**：只标行首的**缩进**。缩进是"这段属于这个块"的视觉标记，
 *   把后面的正文也染上，只会让文字更难读；
 * - **嵌套的头行**只算头行、不算外层的块内：两种着色叠在同一行上，谁赢取决于样式顺序，
 *   那是碰运气，不是规则。
 */
export function templateMarks(lines: string[]): TemplateMark[] {
  const heads: number[] = [];
  for (let index = 0; index < lines.length; index += 1) {
    if (isTemplateHead(lines[index] ?? "")) {
      heads.push(index);
    }
  }
  const isHead = new Set(heads);
  // 一行可能同时落在多个块的范围内（嵌套），但它的缩进标记只有一条 —— 去重
  const markedBodies = new Set<number>();

  const marks: TemplateMark[] = [];
  for (const index of heads) {
    const text = lines[index] ?? "";
    marks.push({ line: index, head: true, start: 0, end: text.length });

    const last = templateBlockEnd(lines, index, headIndent(text));
    for (let inner = index + 1; inner <= last; inner += 1) {
      if (isHead.has(inner) || markedBodies.has(inner)) {
        continue;
      }
      const text = lines[inner] ?? "";
      // 空行不标 —— **哪怕它带着空白**。打字时很容易留下几个空格，
      // 而按"缩进大于零"来判就会给这种行画出一条孤零零的色块，
      // 看上去像块在这里断了（其实没有）。
      if (isBlank(text)) {
        continue;
      }
      const indent = headIndent(text);
      if (indent === 0) {
        // 没有缩进就没有可标的列；零长度装饰会直接抛异常
        continue;
      }
      marks.push({ line: inner, head: false, start: 0, end: indent });
      markedBodies.add(inner);
    }
  }

  marks.sort((a, b) => a.line - b.line || a.start - b.start);
  return marks;
}

/** 一条装饰的**文档偏移量**（从全文开头算起） */
export interface TemplateRange {
  from: number;
  to: number;
  head: boolean;
}

/**
 * 把 `templateMarks` 的行列换算成文档偏移量。
 *
 * 这一步单独抽出来：面板能算出与 DOM 里实际有的东西一致的结果，"规则错"还是
 * "装饰没画上"当场就能分清。
 *
 * 换算是按全文逐字符累加（换行算一个字符）。
 */
export function templateRanges(lines: string[]): TemplateRange[] {
  const starts: number[] = [];
  let offset = 0;
  for (const text of lines) {
    starts.push(offset);
    offset += text.length + 1; // 加上被 split 掉的那个换行
  }

  const ranges: TemplateRange[] = [];
  for (const mark of templateMarks(lines)) {
    const start = starts[mark.line] ?? 0;
    const length = (lines[mark.line] ?? "").length;
    const from = start + Math.min(mark.start, length);
    const to = start + Math.min(mark.end, length);
    if (to <= from) {
      continue;
    }
    ranges.push({ from, to, head: mark.head });
  }
  return ranges;
}

/**
 * 模板块覆盖的**全部行**（1 基不方便，这里用 0 基），**包含块内的空行**。
 *
 * 与 `templateMarks` 的区别就在这里：标记是"给这一行的某些列上色"，空行没有列可上色，
 * 于是块在空行处会断一条缝；而这一份是给**整行**加装饰用的 —— 空行也算，
 * 于是块的左边缘能一路画下去，跨空行是**连续**的。
 */
export function templateBlockLines(lines: string[]): number[] {
  const covered = new Set<number>();
  for (let index = 0; index < lines.length; index += 1) {
    const text = lines[index] ?? "";
    if (!isTemplateHead(text)) {
      continue;
    }
    covered.add(index);
    const last = templateBlockEnd(lines, index, headIndent(text));
    for (let inner = index + 1; inner <= last; inner += 1) {
      covered.add(inner);
    }
  }
  return [...covered].sort((a, b) => a - b);
}

/**
 * 模板块的**折叠范围**：`index` 是头行（0 基）时给出块内行的区间（0 基，含端点）。
 *
 * 默认的 markdown 折叠按结构走（段落、标题…），而模板块对它只是一段普通文字 ——
 * 于是折到第一个空行就停了，跟渲染的"跨空行"对不上。
 *
 * 头行没有内容可折时返回 `null`（没有内容的行不该出现折叠箭头）。
 */
export function templateFoldRange(
  lines: string[],
  index: number,
): { from: number; to: number } | null {
  const text = lines[index] ?? "";
  if (!isTemplateHead(text)) {
    return null;
  }
  const last = templateBlockEnd(lines, index, headIndent(text));
  if (last <= index) {
    return null;
  }
  return { from: index + 1, to: last };
}
