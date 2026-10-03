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
import { computed } from "vue";
import { freshUrl } from "../../dom/file-unlock.ts";
import type { FileInfo } from "../../ipc/files.ts";

/**
 * 附件的**就地预览**：读得动之后摆的东西。
 *
 * 为什么单独一个组件：这一段是文件页里唯一一处"看着内容"的地方，它要回答的是
 * 「这一份是什么、能不能就地看」，与页面其余部分（页头、元信息、动作条）说的是
 * 另一件事。按 mime 前缀分四类，能预览的就地预览，不能预览的给一句实话 ——
 * 说实话而不是给一个坏掉的播放器，是这一段存在的意义。
 *
 * 图片那一处为什么用 `freshUrl`：地址里已经带着版本号（看历史时取的是那一版的字节），
 * 而 webview 会拿旧缓存顶上来；`dom/file-unlock.ts` 那条规矩就是为这个准备的。
 * 音视频不套它 —— 那两个元素本来就要按当前地址缓冲。
 */
const props = defineProps<{
  /** 这一份的现状（页面已确认读得动） */
  entry: FileInfo;
}>();

/** 能就地预览的几类：按 mime 前缀分，其余一律"不能预览" */
const isImage = computed(() => props.entry.mime.startsWith("image/"));
const isVideo = computed(() => props.entry.mime.startsWith("video/"));
const isAudio = computed(() => props.entry.mime.startsWith("audio/"));
const isText = computed(() => props.entry.mime.startsWith("text/"));
</script>

<template>
  <figure v-if="isImage" class="file__preview">
    <!-- 地址里已经带着版本号（看历史时取的是那一版的字节）；
         `freshUrl` 再加一个时间戳绕开 webview 的缓存 -->
    <img :src="freshUrl(entry.url)" :alt="entry.name"/>
  </figure>
  <figure v-else-if="isVideo" class="file__preview">
    <video :src="entry.url" controls preload="metadata"/>
  </figure>
  <figure v-else-if="isAudio" class="file__preview">
    <audio :src="entry.url" controls preload="metadata"/>
  </figure>
  <p v-else-if="isText" class="file__note">这是文本文件，另存为之后可以打开查看。</p>
  <p v-else class="file__note">
    这个类型不能在这里预览。可以交给系统的默认应用打开，或另存为。
  </p>
</template>

<style scoped>
.file__preview {
  margin: 16px 0 0;
}

.file__preview img,
.file__preview video {
  max-width: 100%;
  max-height: 70vh;
  border: 1px solid var(--border);
  border-radius: 8px;
}

.file__preview audio {
  width: 100%;
}

.file__note {
  margin: 16px 0 0;
  color: var(--text-dim);
  font-size: 13.5px;
}
</style>