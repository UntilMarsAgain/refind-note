//! `::html` 的净化器：**按 HTML5 规范解析，再按白名单逐个节点重建**。
//!
//! 之前那份是手写的字符串扫描（见 git 历史），它自己文档里就写着"这不是一个完整的
//! HTML 净化器"：它只认 `<script>`、`on*=`、`javascript:` 三样。真正的净化器要面对的
//! 远不止这些 —— 大小写与属性顺序的绕过、`java\0script:`、`<svg>` 里的外来内容、
//! `srcdoc`、畸形标签让浏览器与净化器对同一段文本产生不同理解…… 这些都不是"多写几个
//! if"能补上的。
//!
//! 所以这里换一套做法：**用浏览器同一套的解析器（[`html5ever`]，WHATWG HTML5 规范）
//! 先把 HTML 变成一棵树，再自己按白名单走一遍、把树重新拼成字符串**。三件事因此一起成立：
//!
//! - **只有白名单里的东西能活下来**，不是"黑名单里那些被拿掉"。没想过的东西默认
//!   出不去 —— 这是净化器该有的默认方向。
//! - **输出是我们自己拼的**，不是把输入原样吐回去。拼的时候属性值与文本都重新转义，
//!   所以不存在"原样穿过来了"这种可能。
//! - **理解与浏览器一致**：解析���实的畸形标签（`<b><i></b></i>`、未闭合的 `<p>`）
//!   按规范补成浏览器看到的那棵树，我们与浏览器看的是同一份东西。
//!
//! ## 白名单的取舍
//!
//! **按整棵子树丢掉**的：`script` / `style` / `link` / `meta` / `base` / `object` /
//! `embed` / `applet` / `frame` / `frameset` / `form` 及其控件 / `template` / `noscript` /
//! `svg` / `math`。最后两个是**外来内容**（foreign content）—— 它们有自己的一套命名空间
//! 规则，是绕过白名单的经典入口，不是因为"画图"就丢的。
//!
//! **不保留 `style` 属性**：内联样式能 `url()` 出去（能追踪读了哪一页）、能有
//! 表达式、能改布局把东西藏起来。要样式就用 `::css` —— 那才是样式该待的地方，
//! 它与 `::html` 分开正是这个道理。
//!
//! **URL 只认几个协议**：`http` / `https` / `mailto`，以及相对地址与 `#片段`。
//! `javascript:` / `data:` / `vbscript:` 与一切没听说过的都丢。判定前先按浏览器的
//! 规矩把协议那一段里的空白与控制字符去掉 —— 否则 `java\nscript:` 是绕得过去的。
//!
//! **不给 `id`**：程序自己的锚点与查找用的是 `data-*` 与 class，`id` 撞了会串。
//!
//! ## `<markdown>`：让 HTML 里反过来用本程序自己的方言
//!
//! ```text
//! ::html
//!   <div class="card">
//!     <markdown>
//!     这里仍然是 **markdown**：可以写列表、写 [[内部链接]]，
//!     也可以再嵌一个 ::note 提示框。
//!     </markdown>
//!   </div>
//! ```
//!
//! 做法是**先把 `<markdown>` 那几段从原文里取出来**（原样，一个字节不改），在原处
//! 放一个占位记号；余下的 HTML 照常解析、净化；拼字符串时碰到占位记号，就把那一段
//! 真的渲染一遍 markdown 塞进去。
//!
//! 为什么不用"解析完再从树里取"：那等于让 HTML 解析器先把 markdown 当 HTML 过一遍
//! —— `&` 会被解成实体、`<div>` 会被当成标签吃掉、缩进与空行的意义全变了。markdown
//! 的原文必须**原封不动**地回到自己的解析器手里，那才是同一种语言。
//!
//! 占位记号用私有区的两个字符夹住序号，它们在 HTML 文本里不会自然出现，也不会被
//! 实体解码掉，所以能从解析结果里原样认回来。
//!
//! 套得深了会停：`<markdown>` 里再来一个 `::html`，里面又有 `<markdown>`……
//! 用 [`crate::markdown::deeper`] 记层数，超了就渲染一句说明而不是继续。

use std::fmt::Write;

use html5ever::tendril::TendrilSink;
use html5ever::ParseOpts;
use markup5ever_rcdom::{Handle, NodeData, RcDom};

/// `<markdown>` 最多套这么多层。
///
/// 它防的是**模板自己套自己**（`::html` → `<markdown>` → `::html` → …），不是性能闸门。
/// 比模板嵌套（`MAX_TEMPLATE_DEPTH`）少：每套一层要重新解析一遍 markdown 并净化一遍
/// HTML，而且这条路的输入是作者写的 HTML —— 更该早点停。
///
/// 顺带说清楚它**不管**什么：限层数限不掉"嵌套的 `::` 块里有内容就很慢"那件事 ——
/// 那是块规则本身的问题（见 [`crate::markdown::syntax::template::scanner`] 里
/// `state.md.parse` 那段），与这条无关。曾经在这里加过一个"按产出字节记账"的额度，
/// 实测**一次也没用上**（跑完的产出只有几百字节），所以撤掉了。
const MAX_NESTING: usize = 8;

/// 净化一段 HTML（`::html` 的正文）。
///
/// 白名单与取舍写在模块头。这里只讲**顺序**，因为顺序是这个实现的关键：
///
/// 1. 先把 `<markdown>` 那几段取出来，原处留占位记号；
/// 2. 剩下的按 HTML5 规范解析成树；
/// 3. 走一遍树，白名单外的丢、白名单里的按规则重建，占位记号处把渲染好的 markdown 塞进去。

pub fn sanitize(source: &str) -> String {
    let (skeleton, slots) = lift_markdown(source);
    let dom = html5ever::parse_document(RcDom::default(), ParseOpts::default())
        .from_utf8()
        .one(skeleton.as_bytes());
    let mut out = String::with_capacity(skeleton.len() + 64);
    walk(&dom.document, &slots, &mut out);
    out
}

// ---------------------------------------------------------------- <markdown> 那一段

/// 占位记号的两头：私有区的字符，HTML 文本里不会自然出现，也躲得开实体解码。
const SLOT_OPEN: char = '\u{E000}';
const SLOT_CLOSE: char = '\u{E001}';

/// 把 `<markdown>…</markdown>` 那几段取出来，原处留下占位记号。
///
/// 返回 `(骨架, 各段的原文)`。**取出来的是原样的字节** —— markdown 后面还要回到
/// 自己的解析器手里，任何"顺手规范化一下"都是错的。
///
/// 没有闭合标签时，那一段就当普通文字留着（骨架里原样不动）—— 少渲染一点比错渲染好。
fn lift_markdown(source: &str) -> (String, Vec<String>) {
    let lower = source.to_ascii_lowercase();
    let mut out = String::with_capacity(source.len());
    let mut slots: Vec<String> = Vec::new();
    let mut cursor = 0usize;

    while let Some(open) = find_tag(&lower, cursor, "markdown") {
        let (body_from, close_tag) = match find_close(&lower, open) {
            Some(pair) => pair,
            None => break,
        };
        // 开标签本身整段丢掉（含它那些属性）：`<markdown>` 不该有属性
        let body_end = close_tag;
        let body = source[body_from..body_end].to_string();
        let slot = slots.len();
        slots.push(body);

        out.push_str(&source[cursor..open]);
        let _ = write!(out, "{SLOT_OPEN}{slot}{SLOT_CLOSE}");
        cursor = body_end;
    }
    out.push_str(&source[cursor..]);
    (out, slots)
}

/// 从 `from` 起找 `<markdown` 这个开标签，返回它 `<` 的位置。
fn find_tag(haystack: &str, from: usize, name: &str) -> Option<usize> {
    let mut at = from;
    while let Some(found) = haystack[at..].find('<') {
        let start = at + found;
        let rest = &haystack[start + 1..];
        if !rest.starts_with(name) {
            at = start + 1;
            continue;
        }
        // 标签名后面必须是空白、`>` 或 `/` —— 否则那是 `<markdowner>` 之类
        let after = rest[name.len()..].chars().next();
        if matches!(after, None | Some(' ') | Some('\t') | Some('\n') | Some('\r') | Some('>'))
            || rest[name.len()..].starts_with('/')
        {
            return Some(start);
        }
        at = start + 1;
    }
    None
}

/// `<markdown>` 的正文从哪儿开始，以及与它配对的那个 `</markdown…>` 的 `<` 在哪儿。
///
/// **要数层数**：`<markdown>` 里可以再写 `<markdown>`，只找第一个闭合标签会把内层的
/// 开标签当正文、把外层的闭合当结束 —— 结果整段都散了。正文也就没有"原样"可言了。
fn find_close(haystack: &str, open: usize) -> Option<(usize, usize)> {
    // 关键：**从开标签之后**开始数。数到 `depth` 归零的那个 `</markdown>` 才是
    // 与最外层配对的那个 —— 从开标签本身那个位置开始数的话，开标签会被数两次，
    // 层数永远归不了零（于是整段当作"没闭合"）。
    let body_from = skip_tag(haystack, open);
    let mut depth = 0usize;
    let mut at = body_from;
    loop {
        let next_open = find_tag(haystack, at, "markdown");
        let next_close = find_tag(haystack, at, "/markdown");
        // 哪个先来就走哪个。分开写而不用一个带 `o < c` 的守卫，是为了让编译器
        // 看得清"四种组合都覆盖了" —— 那种写法一旦漏一臂，错误信息很难懂。
        match (next_open, next_close) {
            (Some(o), Some(c)) if o < c => {
                depth += 1;
                at = skip_tag(haystack, o);
            }
            (Some(o), None) => {
                depth += 1;
                at = skip_tag(haystack, o);
            }
            // 闭合标签在前：出一层；`depth` 归零的那个就是与最外层配对的那个
            (_, Some(c)) => {
                at = c;
                if depth == 0 {
                    return Some((body_from, at));
                }
                depth -= 1;
                at = skip_tag(haystack, c);
            }
            // 后面再没有标签了：没闭合，当作没有这一段
            (None, None) => return None,
        }
    }
}

/// 跳过一个标签：越过它的 `>`。
///
/// 找 `>` 时**要跳过属性里的引号** —— `class="a>b"` 里那个 `>` 不是标签的结束。
fn skip_tag(haystack: &str, start: usize) -> usize {
    let bytes = haystack.as_bytes();
    let mut at = start + 1;
    let mut quote: Option<u8> = None;
    while at < bytes.len() {
        let byte = bytes[at];
        match quote {
            Some(open) if byte == open => quote = None,
            Some(_) => {}
            None if byte == b'"' || byte == b'\'' => quote = Some(byte),
            None if byte == b'>' => return at + 1,
            None => {}
        }
        at += 1;
    }
    haystack.len()
}

// ---------------------------------------------------------------- 走树

fn walk(handle: &Handle, slots: &[String], out: &mut String) {
    let children = handle.children.borrow();
    for child in children.iter() {
        walk_node(child, slots, out);
    }
}

fn walk_node(node: &Handle, slots: &[String], out: &mut String) {
    match &node.data {
        NodeData::Document => walk(node, slots, out),

        // 注释与 doctype 一律不要：注释能藏条件渲染的 payload，doctype 只对老页面有意义
        NodeData::Comment { .. } | NodeData::Doctype { .. } => {}

        NodeData::Text { contents } => {
            write_text(&contents.borrow(), slots, out);
        }

        NodeData::Element { name, attrs, .. } => {
            let tag: String = name.local.to_string();
            // 先按小写找：HTML 标签名不区分大小写，而解析器已经把本地名归一过了
            if skip_subtree(&tag) {
                return;
            }
            let Some(rules) = allowed(&tag) else {
                // 不在白名单里的标签：**标签本身丢掉，children 留下**。
                // 这是净化器的常态 —— 作者写了个没人认得的自定义标签，里面的正文
                // 不该跟着一起消失。
                walk(node, slots, out);
                return;
            };
            let open = format!("<{tag}");
            out.push_str(&open);
            for attr in attrs.borrow().iter() {
                let attr_name: String = attr.name.local.to_string();
                let attr_name = attr_name.to_ascii_lowercase();
                let Some(kept) = keep_attribute(&attr_name, rules, &attr.value) else {
                    continue;
                };
                // 布尔属性只写名字：`<details open>` 不是 `<details open="">`，
                // 而输出是我们自己拼的，就拼成作者写下的样子
                if BOOLEAN.contains(&attr_name.as_str()) {
                    let _ = write!(out, " {attr_name}");
                } else {
                    let _ = write!(out, " {attr_name}=\"{}\"", escape_attribute(&kept));
                }
            }
            out.push('>');
            if !is_void(&tag) {
                walk(node, slots, out);
                let _ = write!(out, "</{tag}>");
            }
        }

        // ProcessingInstruction 之类：HTML 里不该出现，丢了
        _ => {}
    }
}

/// 文本节点：把占位记号认出来，认出来了就把那一段 markdown 真的渲染一遍塞进去。
fn write_text(text: &str, slots: &[String], out: &mut String) {
    let mut rest = text;
    loop {
        let Some(open) = rest.find(SLOT_OPEN) else {
            out.push_str(&escape_text(rest));
            return;
        };
        let after = &rest[open + SLOT_OPEN.len_utf8()..];
        let Some(close) = after.find(SLOT_CLOSE) else {
            out.push_str(&escape_text(rest));
            return;
        };
        out.push_str(&escape_text(&rest[..open]));
        let index: String = after[..close].to_string();
        match index.trim().parse::<usize>().ok().and_then(|i| slots.get(i)) {
            Some(body) => render_slot(body, out),
            None => {
                // 记号对不上任何一段：当普通文字留着，别把作者写的东西弄丢
                out.push_str(&escape_text(&rest[open..open + SLOT_OPEN.len_utf8() + close + 1]));
            }
        }
        rest = &after[close + SLOT_CLOSE.len_utf8()..];
    }
}

/// 真的把那一段 markdown 渲染成 HTML，原样写进 `out`（**不再转义** —— 它已经是 HTML）。
fn render_slot(body: &str, out: &mut String) {
    // 层数上限：模板自己套自己时在这里停
    if crate::markdown::template_depth() >= MAX_NESTING {
        out.push_str(&stopped(&format!(
            "&lt;markdown&gt; 套了超过 {MAX_NESTING} 层，已经停在这里（是不是自己套自己？）"
        )));
        return;
    }    // `render_nested` 沿用当前这一趟的链接解析器与模板页表，
    // 所以里面的内部链接不会一律变红链、里面的 `::html src=` 也还能取到页
    let html = crate::markdown::deeper(|| crate::markdown::render_nested(body));
    out.push_str(&html);
}

/// "停在这里"那个框。**已经排好版的 HTML**，所以几处 `&lt;` 是直接写进去的。
fn stopped(why: &str) -> String {
    format!("<div class=\"template template--problem\"><p class=\"template__why\">{why}</p></div>")
}

// ---------------------------------------------------------------- 白名单

/// 全局都允许的那几个。
///
/// `class` 留着是刻意的：作者的模板要能挂自己那套样式（写在与正文同一层的 `::css`）。
/// **`id` 不给** —— 程序自己的锚点与查找用的是 `data-*` 与 class，`id` 撞了会串。
const COMMON: &[&str] = &["class", "title", "lang", "dir"];

/// 布尔属性：只写名字、不带值。
///
/// `<details open>` 与 `<details open="">` 在 HTML 里是一回事，但输出成
/// `open=""` 读起来像值被清空了 —— 输出是我们自己拼的，就拼成作者写的样子。
const BOOLEAN: &[&str] = &["open", "controls", "loop", "muted", "reversed"];

/// 白名单本体：`(标签, 它能带的属性)`。
///
/// 属性是**逐标签**给的，通配一份 `*` 是最容易出事的地方：`img` 拿到 `href` 就成了
/// 链接、`a` 拿到 `src` 就成了脚本载体、`td` 拿到 `srcset` 毫无意义。
///
/// 表里没有的标签 → 标签本身丢掉、内容留下（见 [`walk_node`]）。
const RULES: &[(&str, &[&str])] = &[
    // —— 分块与分区
    ("address", COMMON),
    ("article", COMMON),
    ("aside", COMMON),
    ("blockquote", &["class", "title", "lang", "dir", "cite"]),
    ("details", &["class", "title", "lang", "dir", "open"]),
    ("div", COMMON),
    ("dl", COMMON),
    ("dt", COMMON),
    ("dd", COMMON),
    ("figcaption", COMMON),
    ("figure", COMMON),
    ("footer", COMMON),
    ("hgroup", COMMON),
    ("header", COMMON),
    ("h1", COMMON),
    ("h2", COMMON),
    ("h3", COMMON),
    ("h4", COMMON),
    ("h5", COMMON),
    ("h6", COMMON),
    ("hr", COMMON),
    ("li", &["class", "title", "lang", "dir", "value"]),
    ("main", COMMON),
    ("nav", COMMON),
    ("ol", &["class", "title", "lang", "dir", "start", "reversed", "type"]),
    ("p", COMMON),
    ("section", COMMON),
    ("summary", COMMON),
    ("ul", COMMON),
    // —— 行内
    ("a", &["class", "title", "lang", "dir", "href", "target", "rel", "download"]),
    ("abbr", COMMON),
    ("b", COMMON),
    ("bdi", COMMON),
    ("bdo", &["class", "title", "lang", "dir"]),
    ("cite", COMMON),
    ("code", COMMON),
    ("data", &["class", "title", "lang", "dir", "value"]),
    ("del", &["class", "title", "lang", "dir", "cite", "datetime"]),
    ("dfn", COMMON),
    ("em", COMMON),
    ("i", COMMON),
    ("ins", &["class", "title", "lang", "dir", "cite", "datetime"]),
    ("kbd", COMMON),
    ("mark", COMMON),
    ("q", &["class", "title", "lang", "dir", "cite"]),
    ("rp", COMMON),
    ("rt", COMMON),
    ("ruby", COMMON),
    ("s", COMMON),
    ("samp", COMMON),
    ("small", COMMON),
    ("span", COMMON),
    ("strong", COMMON),
    ("sub", COMMON),
    ("sup", COMMON),
    ("time", &["class", "title", "lang", "dir", "datetime"]),
    ("u", COMMON),
    ("var", COMMON),
    ("wbr", COMMON),
    ("br", COMMON),
    // —— 表格
    ("caption", COMMON),
    ("col", &["class", "title", "lang", "dir", "span"]),
    ("colgroup", &["class", "title", "lang", "dir", "span"]),
    ("table", COMMON),
    ("tbody", COMMON),
    ("td", &["class", "title", "lang", "dir", "colspan", "rowspan", "headers", "abbr"]),
    ("tfoot", COMMON),
    ("th", &["class", "title", "lang", "dir", "colspan", "rowspan", "headers", "abbr", "scope"]),
    ("thead", COMMON),
    ("tr", COMMON),
    // —— 代码与预格式化（`::code` 那类自定义标记是靠 class 挂在这层上的）
    ("pre", COMMON),
    // —— 图片与音视频
    ("img", &["class", "title", "lang", "dir", "src", "alt", "width", "height", "loading"]),
    ("picture", COMMON),
    ("source", &["class", "title", "lang", "dir", "src", "srcset", "type", "media"]),
    ("track", &["class", "title", "lang", "dir", "src", "kind", "srclang", "label"]),
    ("audio", &["class", "title", "lang", "dir", "src", "controls", "loop", "muted", "preload"]),
    (
        "video",
        &[
            "class", "title", "lang", "dir", "src", "controls", "loop", "muted", "preload",
            "poster", "width", "height",
        ],
    ),
];

/// 按标签名查属性规则。`None` = 不在白名单里。
fn allowed(tag: &str) -> Option<&'static [&'static str]> {
    RULES.iter().find(|(name, _)| *name == tag).map(|(_, attrs)| *attrs)
}

/// 这些标签**连内容一起**丢掉。
///
/// 与"不认识"的分界在这里：上面那些是"标签没了、内容留下"，这里是"内容可能正是
/// 危险的那一份" —— `<script>` 的内容是代码、`<style>` 的内容是能外联的 CSS、
/// `<svg>`/`<math>` 有自己的命名空间规则。
fn skip_subtree(tag: &str) -> bool {
    matches!(
        tag,
        "script" | "style"
            | "link"
            | "meta"
            | "base"
            | "object"
            | "embed"
            | "applet"
            | "frame"
            | "frameset"
            | "noscript"
            | "template"
            | "form"
            | "input"
            | "button"
            | "select"
            | "option"
            | "optgroup"
            | "textarea"
            | "label"
            | "fieldset"
            | "legend"
            | "datalist"
            | "output"
            | "progress"
            | "meter"
            | "svg"
            | "math"
            | "canvas"
            | "audio-g"
    )
}

/// 不用写结束标签的那些。
fn is_void(tag: &str) -> bool {
    matches!(
        tag,
        "area" | "base" | "br" | "col" | "embed" | "hr" | "img" | "input" | "link" | "meta"
            | "param" | "source" | "track" | "wbr"
    )
}

/// 属性留不留、拿什么值出去。`None` = 丢掉。
///
/// 返回的是**收拾过的值**：协议在这里验过，`target` 收窄过。要单独写出来而不是
/// 让调用处自己拼，是因为"哪些属性需要特别看一眼"这件事，得在一处说得清。
fn keep_attribute(name: &str, rules: &[&str], value: &str) -> Option<String> {
    if !rules.contains(&name) {
        return None;
    }
    // `style` 一律不留（模块头写了为什么）；`on*` 本来就不在表里，这里再兜一层，
    // 免得以后往表里加属性时手滑加了 `onclick`
    if name == "style" || name.starts_with("on") {
        return None;
    }
    match name {
        "href" | "src" | "srcset" | "cite" | "poster" | "download" => safe_url(value).then(|| value.to_string()),
        // 只认 `_blank` / `_self`：`_top` 与框架名能把人带走，而这里没有框架
        "target" => matches!(value, "_blank" | "_self").then(|| value.to_string()),
        "rel" => Some(value.to_string()),
        _ => Some(value.to_string()),
    }
}

/// 这个地址能不能放行。
///
/// 只认 `http` / `https` / `mailto` 与相对地址（含 `#片段`）。判定之前先照浏览器的
/// 规矩把协议那一段里的**空白与控制字符**去掉 —— 否则 `java\nscript:alert(1)` 与
/// ` javascript:` 都是绕得过去的，而浏览器认得它们。
fn safe_url(value: &str) -> bool {
    // 相对地址与片段没有协议这一段，直接放行
    let probe: String = value
        .chars()
        .filter(|ch| !ch.is_whitespace() && !ch.is_control())
        .collect();
    let Some((scheme, _)) = probe.split_once(':') else {
        return true; // 没有协议：相对地址或 `#片段`
    };
    // `split_once` 给的 scheme 里可能混着 `/`、`?`、`#` —— 那就不是协议了，是相对地址
    if scheme.contains(['/', '?', '#']) {
        return true;
    }
    let scheme = scheme.to_ascii_lowercase();
    matches!(scheme.as_str(), "http" | "https" | "mailto")
}

// ---------------------------------------------------------------- 转义

/// 文本节点的转义：`&` `<` `>`。
fn escape_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(ch),
        }
    }
    out
}

/// 属性值的转义：文本那三样之外还要管 `"`（我们用双引号包值）。
fn escape_attribute(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clean(source: &str) -> String {
        sanitize(source)
    }

    /// 最要紧的一条：白名单是**方向**，没想过的东西默认出不去
    #[test]
    fn unknown_elements_lose_their_tags_but_keep_their_words() {
        let html = clean("<my-widget>里面的正文</my-widget>");
        assert!(!html.contains("my-widget"), "{html}");
        assert!(html.contains("里面的正文"), "{html}");
    }

    #[test]
    fn scripts_are_gone_with_their_bodies() {
        let html = clean(r#"<p>前</p><script>alert(1)</script><p>后</p>"#);
        assert!(!html.contains("script"), "{html}");
        assert!(!html.contains("alert"), "{html}");
        assert!(html.contains("前") && html.contains("后"), "{html}");
    }

    #[test]
    fn styles_and_foreign_content_are_gone_with_their_bodies() {
        for source in [
            "<style>body{display:none}</style>",
            "<svg><script>alert(1)</script></svg>",
            "<math><mtext><script>alert(1)</script></mtext></math>",
            "<object data=\"x\"></object>",
            "<form action=\"/x\"><input name=\"a\"></form>",
        ] {
            let html = clean(source);
            assert!(!html.contains("alert"), "{source}: {html}");
            assert!(!html.contains("<svg"), "{source}: {html}");
            assert!(!html.contains("display:none"), "{source}: {html}");
        }
    }

    #[test]
    fn inline_style_attributes_never_survive() {
        let html = clean(r#"<p style="background:url(//tracker)">字</p>"#);
        assert!(!html.contains("style"), "{html}");
        assert!(!html.contains("tracker"), "{html}");
        assert!(html.contains("字"), "{html}");
    }

    #[test]
    fn javascript_urls_are_refused_however_they_are_disguised() {
        for source in [
            r#"<a href="javascript:alert(1)">点</a>"#,
            r#"<a href="JaVaScRiPt:alert(1)">点</a>"#,
            "<a href=\"java\nscript:alert(1)\">点</a>",
            r#"<a href=" javascript:alert(1)">点</a>"#,
            r#"<a href="vbscript:msgbox(1)">点</a>"#,
            r#"<a href="data:text/html,<script>alert(1)</script>">点</a>"#,
        ] {
            let html = clean(source);
            assert!(!html.contains("javascript"), "{source}: {html}");
            assert!(!html.contains("vbscript"), "{source}: {html}");
            assert!(!html.contains("data:"), "{source}: {html}");
            assert!(!html.contains("script"), "{source}: {html}");
        }
    }

    #[test]
    fn ordinary_urls_and_relative_targets_stay() {
        let html = clean(r#"<a href="https://example.com/x?y=1">外链</a>"#);
        assert!(html.contains(r#"href="https://example.com/x?y=1""#), "{html}");

        let relative = clean(r#"<a href="另一页">内部</a>"#);
        assert!(relative.contains("另一页"), "{relative}");

        let fragment = clean(r##"<a href="#安装">跳节</a>"##);
        assert!(fragment.contains("#安装"), "{fragment}");
    }

    #[test]
    fn event_attributes_and_unknown_ones_are_dropped() {
        let html = clean(r#"<p onclick="alert(1)" onmouseover="x()" data-x="1" id="锚">字</p>"#);
        assert!(!html.contains("onclick"), "{html}");
        assert!(!html.contains("onmouseover"), "{html}");
        assert!(!html.contains("data-x"), "{html}");
        assert!(!html.contains("id="), "{html}");
        assert!(html.contains("字"), "{html}");
    }

    /// 畸形标签：净化器与浏览器得看到同一棵树，否则净化等于没用
    #[test]
    fn malformed_markup_is_read_the_way_a_browser_reads_it() {
        // 未闭合的 <b>：浏览器认为它包住了后面全部内容
        let html = clean("<b>粗<p>段落</b>后面");
        assert!(html.contains("<b>"), "{html}");
        assert!(html.contains("</b>"), "{html}");
        assert!(html.contains("<p>"), "{html}");
    }

    #[test]
    fn attribute_values_are_re_escaped_on_the_way_out() {
        let html = clean(r#"<p title='a"b<c&d'>字</p>"#);
        assert!(!html.contains(r#"title="a"b"#), "{html}");
        assert!(html.contains("&quot;") || html.contains("&#34;"), "{html}");
        assert!(html.contains("&lt;c"), "{html}");
        assert!(html.contains("&amp;d"), "{html}");
    }

    #[test]
    fn comments_and_doctypes_do_not_survive() {
        let html = clean("<!doctype html><!-- secret --><p>正文</p>");
        assert!(!html.contains("secret"), "{html}");
        assert!(!html.contains("doctype"), "{html}");
        assert!(html.contains("正文"), "{html}");
    }

    /// `<markdown>` 那一段：确实回到自己的解析器手里了
    #[test]
    fn a_markdown_element_is_rendered_by_the_markdown_renderer() {
        let html = clean("<div><markdown>**加粗**与 [[某页]]</markdown></div>");
        assert!(html.contains("<strong>加粗</strong>"), "{html}");
        // 内部链接是 markdown-it 的 wikilink，不是原样留下的方括号
        assert!(html.contains("wikilink"), "{html}");
        assert!(!html.contains("<markdown"), "{html}");
    }

    /// markdown 那段里的模板也要能用 —— 它就是回到同一个解析器
    #[test]
    fn markdown_inside_markdown_still_gets_templates() {
        let html = clean("<markdown>\n::note title=\"嵌的\"\n  正文\n</markdown>");
        assert!(html.contains("callout--note"), "{html}");
        assert!(html.contains("嵌的"), "{html}");
    }

    /// 关键是**原样**：取出来的那一段必须与原文**逐字节相同**。
    ///
    /// 之前这个性质是靠"渲染出来的 HTML 长什么样"来验的，结果两次都验错 ——
    /// markdown 与 HTML 对 `&lt;`、注释、`<b>` 这几样的最终处理**恰好一致**，
    /// 两条路殊途同归，从输出上根本分不出来。所以直接验中间那一步。
    #[test]
    fn the_lifted_markdown_is_byte_identical_to_the_source() {
        let body = "写的是 &lt;b&gt; 不是标签\n\n<!-- 注释 -->\n<div 没闭合\n**加粗** & 别的";
        let source = format!("<p>前</p><markdown>{body}</markdown><p>后</p>");

        let (skeleton, slots) = lift_markdown(&source);
        assert_eq!(slots.len(), 1, "{skeleton}");
        assert_eq!(slots[0], body, "取出来的那一段必须与原文逐字节相同");
        // 骨架里不该再留下 `<markdown` 这个标签，只剩占位记号
        assert!(!skeleton.contains("<markdown"), "{skeleton}");
        assert!(skeleton.contains(SLOT_OPEN), "{skeleton}");
        // 标签之外的内容一点没动
        assert!(skeleton.contains("<p>前</p>"), "{skeleton}");
        assert!(skeleton.contains("<p>后</p>"), "{skeleton}");
    }

    /// 嵌着的 `<markdown>` 要配上：最外层取到的应当是**整段内层**，不是第一段
    #[test]
    fn lifting_respects_nesting() {
        let source = "<markdown>外<markdown>内</markdown>尾</markdown>";
        let (_, slots) = lift_markdown(source);
        assert_eq!(slots.len(), 1, "外层与内层配对，应当只有一段");
        assert_eq!(slots[0], "外<markdown>内</markdown>尾");
    }

    /// 生成 `levels` 层 `::html` → `<markdown>` → `::html` → … 的嵌套。
    ///
    /// 注意缩进是**逐层加深的** —— 模板块的边界由缩进划出，写平了就不成块了。
    fn nested_html(levels: usize) -> String {
        let mut out = String::new();
        for depth in 0..levels {
            let pad = "  ".repeat(depth + 1);
            out.push_str(&format!("{pad}::html\n{pad}  <markdown>\n"));
        }
        out.push_str(&format!("{}到底了\n", "  ".repeat(levels + 1)));
        for depth in (0..levels).rev() {
            let pad = "  ".repeat(depth + 1);
            out.push_str(&format!("{pad}  </markdown>\n"));
        }
        out
    }

    /// 一层是正常的：`<markdown>` 里那段的模板与链接都真的生效
    #[test]
    fn a_nested_html_block_still_works() {
        let html = clean("<markdown>\n::html\n  <markdown>\n  内层\n  </markdown>\n</markdown>");
        assert!(html.contains("内层"), "{html}");
        assert!(!html.contains("已经停在这里"), "{html}");
    }

    /// 套得深了要停 —— 而且是**说清楚为什么停**，不是静悄悄截断
    ///
    /// 层数取 12：够越过 [`MAX_NESTING`]（8）触发那道闸，又不至于让这个测试变成
    /// **性能测试**。层数再往上的耗时问题不是这里要管的事 —— 那是块规则本身的性质，
    /// 见 `scanner.rs` 里"嵌套的块里有内容就很慢"那段。
    #[test]
    fn nesting_stops_instead_of_running_forever() {
        let html = clean(&nested_html(12));
        assert!(html.contains("已经停在这里"), "{html}");
    }

    /// 没闭合的 `<markdown>` 当普通文字留着，不吞掉后面所有内容
    #[test]
    fn an_unclosed_markdown_element_does_not_swallow_the_page() {
        let html = clean("<p>前</p><markdown>后面");
        assert!(html.contains("前"), "{html}");
        assert!(html.contains("后面"), "{html}");
    }

    /// 属性是**逐标签**给的：`img` 拿不到 href，`a` 拿不到 src
    #[test]
    fn attributes_are_granted_per_tag() {
        let img = clean(r#"<img src="a.png" alt="图" href="/evil">"#);
        assert!(img.contains(r#"src="a.png""#), "{img}");
        assert!(!img.contains("href"), "{img}");

        let link = clean(r#"<a href="/x" src="/evil">字</a>"#);
        assert!(link.contains("href"), "{link}");
        assert!(!link.contains("src"), "{link}");
    }

    /// 常用的一批要真的能用 —— 白名单不该把正文写作的路堵死
    #[test]
    fn the_common_shapes_come_through_usable() {
        let html = clean(
            r#"<table><thead><tr><th scope="col">甲</th></tr></thead>
               <tbody><tr><td colspan="2">乙</td></tr></tbody></table>
               <ul><li>一</li></ul><blockquote cite="/src"><p>引</p></blockquote>
               <details open><summary>细</summary><p>内</p></details>"#,
        );
        for needle in [
            "<table>", "<thead>", "<th scope=\"col\">", "<td colspan=\"2\">",
            "<ul>", "<li>一</li>", "<blockquote", "<details open>", "<summary>细</summary>",
        ] {
            assert!(html.contains(needle), "少了 {needle}: {html}");
        }
    }

    #[test]
    fn void_elements_do_not_get_a_closing_tag() {
        let html = clean("<p>a<br>b<hr>c</p><img src=\"x.png\">");
        assert!(html.contains("<br>"), "{html}");
        assert!(!html.contains("</br>"), "{html}");
        assert!(!html.contains("</img>"), "{html}");
    }
}
