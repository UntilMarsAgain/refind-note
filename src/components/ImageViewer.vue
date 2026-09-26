<script setup lang="ts">
/**
 * 大图查看器：点笔记里的图片打开，点任意处或按 Esc 关闭。
 *
 * 全局只挂这一份（App 根上）。点击处理在 `note-html.ts` 里随笔记 DOM 一起挂，
 * 所以阅读视图与编辑器预览共用同一套行为。
 */
import { onBeforeUnmount, watch } from "vue";
import { X } from "@lucide/vue";
import { closeImage, viewingImage } from "../image-viewer";

function onKey(event: KeyboardEvent) {
  if (event.key === "Escape") {
    closeImage();
  }
}

// 只在打开时挂键盘监听：常驻一个全局监听没必要
watch(viewingImage, (value) => {
  if (value) {
    window.addEventListener("keydown", onKey);
  } else {
    window.removeEventListener("keydown", onKey);
  }
});

onBeforeUnmount(() => window.removeEventListener("keydown", onKey));
</script>

<template>
  <div v-if="viewingImage" class="viewer" role="dialog" aria-modal="true" @click="closeImage">
    <figure class="viewer__box">
      <img class="viewer__image" :src="viewingImage.url" :alt="viewingImage.alt" />
      <figcaption v-if="viewingImage.alt" class="viewer__caption">
        {{ viewingImage.alt }}
      </figcaption>
    </figure>
    <button class="viewer__close" type="button" aria-label="关闭" @click.stop="closeImage">
      <X :size="18" :stroke-width="2" />
    </button>
  </div>
</template>

<style scoped>
/*
 * 不透明的底：浮层要是半透明，背后的字会透上来，读图时很吵
 * （这条规矩是任务栏那边定下来的，这里同样适用）。
 */
.viewer {
  position: fixed;
  inset: 0;
  z-index: 60;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 24px;
  background: var(--bg);
  cursor: zoom-out;
}

.viewer__box {
  max-width: 100%;
  max-height: 100%;
  margin: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 10px;
}

.viewer__image {
  max-width: min(1200px, 92vw);
  max-height: 84vh;
  object-fit: contain;
  border-radius: 8px;
  background: var(--surface);
}

.viewer__caption {
  max-width: 92vw;
  color: var(--text-dim);
  text-align: center;
}

.viewer__close {
  position: absolute;
  top: 16px;
  right: 16px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 34px;
  height: 34px;
  border: 1px solid var(--border);
  border-radius: 8px;
  background: var(--surface);
  color: var(--text);
  cursor: pointer;
}
</style>
