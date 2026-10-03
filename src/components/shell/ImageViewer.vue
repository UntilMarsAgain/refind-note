<!--
  Refind Note is a note-taking software.
  Copyright (C) 2026 Until Mars Again

  This program is free software: you can redistribute it and/or modify
  it under the terms of the GNU Affero General Public License as published by
  the Free Software Foundation, either version 3 of the License, or
  (at your option) any later version.

  This program is distributed in the hope that it will be useful,
  but WITHOUT ANY WARRANTY; without even the implied warranty of
  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
  GNU Affero General Public License for more details.

  You should have received a copy of the GNU Affero General Public License
  along with this program.  If not, see <http://www.gnu.org/licenses/>.
-->

<script setup lang="ts">
import { computed, onDeactivated } from "vue";
import { X } from "@lucide/vue";
import { closeImage, viewingImage } from "../../dom/image-viewer.ts";
import { saveNameOf, saveVaultFile, savableTitle } from "../../dom/file-save.ts";
import { flash } from "../../core/notice.ts";

/**
 * 看大图：整屏一层，点哪都关。
 *
 * 它挂在**正文里**（`NoteContent.vue`，笔记与帮助页都走那儿），不再挂在应用根上 ——
 * 因为它是 `position: fixed` 的浮层，挂在窗口一级的话，A 栏点开的图会跟着飘到 B 栏。
 *
 * 状态本身仍是 `dom/image-viewer.ts` 里那**一份模块级**的（正文里的图片点一下就调它，
 * 而那些点击处理器是 DOM 级的、拿不到组件实例）。单例配多份实例，所以下面那条
 * `onDeactivated` 是必需的，不是顺手加的。
 */
const image = computed(() => viewingImage.value);

/**
 * 这一页被缓存起来（切到别的标签页）时**收起大图**。
 *
 * 状态是全局一份的：不收的话，切到 B 栏会看到 A 栏点开的那张图 —— 而 B 栏的人
 * 根本不知道自己刚才"看"了什么。按浏览器的脾气，浮层属于打开它的那一页，
 * 离开那一页就该收。
 */
onDeactivated(() => {
    closeImage();
});

/** 能另存为的才给按钮：本仓库的文件可以，外链图片不揽这件事 */
const title = computed(() => (image.value ? savableTitle(image.value.url) : null));

async function saveAs() {
  const current = title.value;
  if (!current) {
    return;
  }
  try {
    const target = await saveVaultFile(current, image.value ? saveNameOf(image.value.url) : current);
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
        <button v-if="title" class="viewer__save" type="button" @click="saveAs">另存为…</button>
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
