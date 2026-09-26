<script setup lang="ts">
import { nextTick, onMounted, ref, watch } from "vue";
import { openUrl } from "@tauri-apps/plugin-opener";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { applyLineNumbers, highlightCode } from "../code-blocks";
import { decorateNoteHtml } from "../note-html";
import { codeLineNumbers } from "../settings";

const props = defineProps<{ html: string }>();

const emit = defineEmits<{
  /** 点了页内锚点：章节由前端自己确定（地址里其余成分都以后端解析为准） */
  (e: "section", id: string): void;
  /** Ctrl/Cmd+点击内部链接：在新标签页打开 */
  (e: "wikilink-new", title: string): void;
  /** 右键内部链接：请求上层弹上下文菜单（带上坐标） */
  (e: "wikilink-menu", payload: { title: string; x: number; y: number }): void;
  (e: "wikilink", payload: { title: string; missing: boolean }): void;
}>();

const rootEl = ref<HTMLElement | null>(null);

const COPY_TEXT = "复制";
const COPIED_TEXT = "已复制";
const FAILED_TEXT = "复制失败";

/**
 * 给每个代码块套一层容器并挂上复制按钮。
 * 按钮必须放在 <pre> 外面：pre 有 overflow:auto，放里面会跟着横向滚动被裁掉；
 * 而且放外面，选中或复制代码时不会把按钮算进去。
 */
function decorateCodeBlocks() {
  const root = rootEl.value;
  if (!root) {
    return;
  }

  // 先上色，再加行号：行号列是按行高摆的，色带也是，顺序不影响，但先色后号更直观
  highlightCode(root);

  for (const pre of root.querySelectorAll("pre")) {
    // 复制按钮要用它取原文（高亮交给上面的 highlightCode）
    const code = pre.querySelector("code");

    const frame = document.createElement("div");
    frame.className = "code-frame";
    pre.replaceWith(frame);
    frame.append(pre);

    const button = document.createElement("button");
    button.type = "button";
    button.className = "code-copy";
    button.title = "复制这段代码";
    button.setAttribute("aria-label", "复制这段代码");
    button.textContent = COPY_TEXT;

    button.addEventListener("click", async () => {
      // 走 Tauri 的剪贴板插件而不是 navigator.clipboard：
      // webview 里的 Web 剪贴板 API 在 Linux 上不保证可用。
      try {
        await writeText(code?.textContent ?? "");
        button.textContent = COPIED_TEXT;
      } catch (error) {
        console.error("复制代码失败:", error);
        button.textContent = FAILED_TEXT;
      }

      window.setTimeout(() => {
        button.textContent = COPY_TEXT;
      }, 1200);
    });

    frame.append(button);
  }

  applyLineNumbers(root);
  // 笔记 HTML 的收尾：解析附件地址、给图片挂上点击看大图、取不到时换一句说明
  decorateNoteHtml(root);
}

onMounted(decorateCodeBlocks);
// v-html 更新完 DOM 才是新的，所以要等一拍
watch(
  () => props.html,
  () => nextTick(decorateCodeBlocks),
);
// 设置页把行号关掉时，已经打开的文章要立刻跟着变
watch(codeLineNumbers, () => {
  if (rootEl.value) {
    applyLineNumbers(rootEl.value);
  }
});

/**
 * 渲染结果里的 <a> 如果放任不管，webview 会直接导航过去，
 * 把整个应用界面冲掉。所以一律拦下来：页内锚点自己滚动，外链交给系统浏览器。
 * 协议白名单由 opener 插件的作用域兜底（只允许 http/https/mailto/tel）。
 */
/**
 * 归一化要交给系统浏览器的地址。
 *
 * 已经带协议的**必须原样**（否则会变成 `https://https://…`）；没写协议的补一个 https，
 * 这样「裸地址」也能点开。判断协议用 RFC 3986 的 `scheme:` 形状，不用字符串前缀猜。
 */
function normalizeUrl(value: string): string {
  const trimmed = value.trim();
  return /^[a-zA-Z][a-zA-Z0-9+.-]*:/.test(trimmed) ? trimmed : `https://${trimmed}`;
}

/**
 * 右键内部链接：请求上层弹菜单。外链的右键交给系统（浏览器的
 * 「在新窗口打开 / 复制链接」更完整），这里只管内部链接。
 */
function onContextMenu(event: MouseEvent) {
  const target = event.target;
  if (!(target instanceof Element)) {
    return;
  }
  const wikiLink = target.closest("a.wikilink");
  // 跨站链接按外链对待：右键交给系统（"在新窗口打开 / 复制链接"更完整）
  if (!wikiLink || wikiLink.hasAttribute("href")) {
    return;
  }

  event.preventDefault();
  const title =
    wikiLink.getAttribute("data-title") ?? wikiLink.getAttribute("data-doc");
  if (title) {
    emit("wikilink-menu", { title, x: event.clientX, y: event.clientY });
  }
}

/**
 * 行内代码：点一下复制。
 *
 * 代码块自带复制按钮（按钮能明确表达"这是可点的"），行内代码没有按钮的位置，
 * 就整段当热区；反馈用一次短暂高亮，不弹提示 —— 正文里不该冒出对话框。
 */
async function copyInlineCode(el: Element, text: string) {
  try {
    // 与代码块的复制按钮走同一条路：Tauri 剪贴板插件，不用 navigator.clipboard
    await writeText(text);
  } catch {
    return;
  }
  el.classList.add("code--copied");
  window.setTimeout(() => el.classList.remove("code--copied"), 600);
}

function onClick(event: MouseEvent) {
  const target = event.target;
  if (!(target instanceof Element)) {
    return;
  }

  // 内部链接没有 href，必须先判它，否则会被下面的 a[href] 分支漏掉。
  // 例外是**跨站链接**：它带 class="wikilink"，但本来就是外链（后端给了真实 href），
  // 所以要当外链打开，而不是在本仓库里跳转。
  const wikiLink = target.closest("a.wikilink");
  if (wikiLink) {
    const href = wikiLink.getAttribute("href");
    if (href) {
      event.preventDefault();
      void openUrl(normalizeUrl(href));
      return;
    }

    event.preventDefault();
    // 目标是否存在是后端渲染时判定的（data-missing）：红链点了也没东西可开
    const title =
      wikiLink.getAttribute("data-title") ?? wikiLink.getAttribute("data-doc");
    if (title) {
      // Ctrl/Cmd + 点击＝在新标签页打开（与浏览器一致）
      if (event.ctrlKey || event.metaKey) {
        emit("wikilink-new", title);
        return;
      }
      emit("wikilink", {
        title,
        missing: wikiLink.getAttribute("data-missing") === "true",
      });
    }
    return;
  }

  const anchor = target.closest("a[href]");
  if (!anchor) {
    // 没落到链接上：再看是不是行内代码（点一下复制）
    onClickCode(event);
    return;
  }

  event.preventDefault();

  const href = anchor.getAttribute("href");
  if (!href) {
    return;
  }

  if (href.startsWith("#")) {
    // markdown-it 会把 href 做 URL 编码（中文锚点会变成 %E8%A1%A8...），
    // 而标题的 id 是未编码的原文，所以必须先解码再查。
    let id = href.slice(1);
    try {
      id = decodeURIComponent(id);
    } catch {
      // 非法转义序列就按原样查
    }

    document.getElementById(id)?.scrollIntoView({ block: "start" });
    // 章节是唯一允许前端自己确定的部分：报给上层，让它叠进地址栏
    emit("section", id);
    return;
  }

  void openUrl(normalizeUrl(href));
}

// 放在最后：链接优先。行内代码（`pre` 之外的 code）点一下就是复制。
function onClickCode(event: MouseEvent) {
  const target = event.target;
  if (!(target instanceof Element)) {
    return;
  }
  const code = target.closest("code");
  if (!code || code.closest("pre")) {
    return;
  }
  const text = code.textContent ?? "";
  if (text) {
    void copyInlineCode(code, text);
  }
}
</script>

<template>
  <article
    ref="rootEl"
    class="note-body"
    v-html="html"
    @click="onClick"
    @contextmenu="onContextMenu"
  />
</template>
