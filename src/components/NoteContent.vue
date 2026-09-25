<script setup lang="ts">
import { openUrl } from "@tauri-apps/plugin-opener";

defineProps<{ html: string }>();

/**
 * 渲染结果里的 <a> 如果放任不管，webview 会直接导航过去，
 * 把整个应用界面冲掉。所以一律拦下来，外链交给系统浏览器。
 * 协议白名单由 opener 插件的作用域兜底（只允许 http/https/mailto/tel）。
 */
function onClick(event: MouseEvent) {
  const target = event.target;
  if (!(target instanceof Element)) {
    return;
  }

  const anchor = target.closest("a[href]");
  if (!anchor) {
    return;
  }

  event.preventDefault();

  const href = anchor.getAttribute("href");
  // 站内锚点交给页面自己处理，不往外丢
  if (!href || href.startsWith("#")) {
    return;
  }

  void openUrl(href);
}
</script>

<template>
  <article class="note-body" v-html="html" @click="onClick" />
</template>
