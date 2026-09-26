<script setup lang="ts">
/**
 * 大图查看器：点笔记里的图片打开，点任意处或按 Esc 关闭。
 *
 * 全局只挂这一份（App 根上）。点击处理在 `note-html.ts` 里随笔记 DOM 一起挂，
 * 所以阅读视图与编辑器预览共用同一套行为。
 */
import { X } from "@lucide/vue";
import { computed, onBeforeUnmount, ref, watch } from "vue";
import { Download } from "@lucide/vue";
import { saveVaultFile } from "../file-save";
import { vaultKeyOf } from "../file-links";
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

/**
 * 能不能另存：只有仓库里的文件可以。
 *
 * 外链图片的字节在别的站上（跨域拿不到），所以对它们不给这个按钮 ——
 * 给一个点了没用的按钮比不给更糟。
 */
const vaultKey = computed(() => (viewingImage.value ? vaultKeyOf(viewingImage.value.url) : null));

const trouble = ref("");

async function keep() {
  const key = vaultKey.value;
  if (!key) {
    return;
  }
  trouble.value = "";
  try {
    await saveVaultFile(key);
  } catch (error) {
    trouble.value = String(error);
  }
}
</script>

<template>
  <div v-if="viewingImage" class="viewer" role="dialog" aria-modal="true" @click="closeImage">
    <figure class="viewer__box">
      <img class="viewer__image" :src="viewingImage.url" :alt="viewingImage.alt" />
      <figcaption v-if="viewingImage.alt" class="viewer__caption">
        {{ viewingImage.alt }}
      </figcaption>
    </figure>
    <div v-if="vaultKey || trouble" class="viewer__tools" @click.stop>
      <button v-if="vaultKey" class="viewer__tool" type="button" @click="keep">
        <Download :size="16" :stroke-width="1.9" />
        另存为
      </button>
      <p v-if="trouble" class="viewer__trouble">{{ trouble }}</p>
    </div>

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

.viewer__tools {
  position: absolute;
  top: 16px;
  left: 16px;
  display: flex;
  align-items: center;
  gap: 10px;
}

.viewer__tool {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 7px 12px;
  border: 1px solid var(--border);
  border-radius: 8px;
  background: var(--surface);
  color: var(--text);
  cursor: pointer;
}

.viewer__trouble {
  margin: 0;
  color: var(--danger);
}
</style>