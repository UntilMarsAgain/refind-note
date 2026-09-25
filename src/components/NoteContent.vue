<script setup lang="ts">
import { nextTick, onMounted, ref, watch } from "vue";
import { openUrl } from "@tauri-apps/plugin-opener";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import hljs from "highlight.js/lib/common";

const props = defineProps<{ html: string }>();

const emit = defineEmits<{
  (e: "wikilink", payload: { title: string; missing: boolean }): void;
}>();

const rootEl = ref<HTMLElement | null>(null);

const COPY_TEXT = "复制";
const COPIED_TEXT = "已复制";
const FAILED_TEXT = "复制失败";

/**
 * markdown-it 只给代码块标上 language-xxx，真正的分词交给 highlight.js。
 * 只引入 common 那一档（36 种常用语言），不把全部语言包打进来。
 */
function highlight(block: HTMLElement) {
  const language = Array.from(block.classList)
    .find((name) => name.startsWith("language-"))
    ?.slice("language-".length);

  // 未注册的语言 hljs 会打警告并跳过，这里先挡掉，避免控制台刷屏
  if (language && hljs.getLanguage(language)) {
    hljs.highlightElement(block);
  }
}

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

  for (const pre of root.querySelectorAll("pre")) {
    const code = pre.querySelector("code");
    if (code instanceof HTMLElement) {
      highlight(code);
    }

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
}

onMounted(decorateCodeBlocks);
// v-html 更新完 DOM 才是新的，所以要等一拍
watch(
  () => props.html,
  () => nextTick(decorateCodeBlocks),
);

/**
 * 渲染结果里的 <a> 如果放任不管，webview 会直接导航过去，
 * 把整个应用界面冲掉。所以一律拦下来：页内锚点自己滚动，外链交给系统浏览器。
 * 协议白名单由 opener 插件的作用域兜底（只允许 http/https/mailto/tel）。
 */
function onClick(event: MouseEvent) {
  const target = event.target;
  if (!(target instanceof Element)) {
    return;
  }

  // 内部链接没有 href，必须先判它，否则会被下面的 a[href] 分支漏掉
  const wikiLink = target.closest("a.wikilink");
  if (wikiLink) {
    event.preventDefault();
    // 目标是否存在是后端渲染时判定的（data-missing）：红链点了也没东西可开
    const title =
      wikiLink.getAttribute("data-title") ?? wikiLink.getAttribute("data-doc");
    if (title) {
      emit("wikilink", {
        title,
        missing: wikiLink.getAttribute("data-missing") === "true",
      });
    }
    return;
  }

  const anchor = target.closest("a[href]");
  if (!anchor) {
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
    return;
  }

  void openUrl(href);
}
</script>

<template>
  <article ref="rootEl" class="note-body" v-html="html" @click="onClick" />
</template>
