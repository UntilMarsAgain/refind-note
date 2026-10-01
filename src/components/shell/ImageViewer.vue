<script setup lang="ts">
import { computed } from "vue";
import { X } from "@lucide/vue";
import { closeImage, viewingImage } from "../../view/image-viewer.ts";
import { saveVaultFile, savableKey } from "../../view/file-save.ts";
import { flash } from "../../core/notice.ts";

/**
 * 看大图：整屏一层，点哪都关。
 *
 * 它挂在应用根上（`App.vue`），所以正文里任何一张图都能调它 —— 阅读页与编辑器预览
 * 用的是同一份状态。
 */
const image = computed(() => viewingImage.value);

/** 能另存为的才给按钮：仓库里的附件可以，外链图片不揽这件事 */
const key = computed(() => (image.value ? savableKey(image.value.url) : null));

async function saveAs() {
  const current = key.value;
  if (!current) {
    return;
  }
  try {
    const target = await saveVaultFile(current);
    if (target) {
      flash(`已另存为：${target}`);
    }
  } catch (error) {
    flash(`另存失败：${error}`);
  }
}
</script>

<template>
  <div
      v-if="image"
      class="viewer"
      role="dialog"
      aria-modal="true"
      aria-label="看大图"
      @click="closeImage"
      @keydown.esc="closeImage"
  >
    <button class="viewer__close" type="button" title="关闭" aria-label="关闭" @click.stop="closeImage">
      <X :size="18" :stroke-width="2"/>
    </button>

    <figure class="viewer__frame" @click.stop>
      <img class="viewer__image" :src="image.url" :alt="image.alt"/>
      <figcaption class="viewer__caption">
        <span v-if="image.alt" class="viewer__alt">{{ image.alt }}</span>
        <button v-if="key" class="viewer__save" type="button" @click="saveAs">另存为…</button>
      </figcaption>
    </figure>
  </div>
</template>

<style scoped>
/* 整屏盖住：看图时别让正文露在旁边 */
.viewer {
  position: fixed;
  inset: 0;
  z-index: 60;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 32px;
  /* 不透明：底下的正文会干扰看图 */
  background: rgb(0 0 0 / 88%);
}

.viewer__frame {
  display: flex;
  flex-direction: column;
  gap: 10px;
  align-items: center;
  margin: 0;
  max-width: 100%;
  max-height: 100%;
}

.viewer__image {
  max-width: min(1200px, 92vw);
  max-height: 84vh;
  object-fit: contain;
}

.viewer__caption {
  display: flex;
  flex-wrap: wrap;
  gap: 12px;
  align-items: center;
  justify-content: center;
  color: rgb(255 255 255 / 78%);
  font-size: 12.5px;
}

.viewer__alt {
  overflow-wrap: anywhere;
}

.viewer__save {
  padding: 4px 12px;
  border: 1px solid rgb(255 255 255 / 35%);
  border-radius: 6px;
  background: transparent;
  color: inherit;
  font: inherit;
  font-size: 12.5px;
  cursor: pointer;
}

.viewer__save:hover {
  border-color: rgb(255 255 255 / 70%);
  color: #fff;
}

.viewer__close {
  position: absolute;
  top: 14px;
  right: 16px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 32px;
  height: 32px;
  padding: 0;
  border: 0;
  border-radius: 6px;
  background: transparent;
  color: rgb(255 255 255 / 75%);
  cursor: pointer;
}

.viewer__close:hover {
  background: rgb(255 255 255 / 14%);
  color: #fff;
}
</style>
