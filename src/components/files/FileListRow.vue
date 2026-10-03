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
/**
 * 文件列表里的**一行**：缩略图、名字（或者改名的输入框）、元信息、那串动作。
 *
 * 为什么单独一个组件：这一行是列表页里唯一会**变出另一种形态**的地方 ——
 * 正常那排按钮 / 改名的输入框 + 确认取消 / 确认删除 + 取消，三个形态挤在一处时
 * 模板里全是 `v-if` 的交叉，扫一眼看不出"此刻这一行能按哪几个键"。
 * 收在一个组件里之后，每个形态就是一个 `v-if` 分支，而"这一行正处在哪个形态"
 * 由外面用 `renaming` / `confirming` / `updating` 三个布尔说给它（那些状态在
 * `useFileList.ts` 里，**按标题记**，所以同一时刻只有一行是特殊的）。
 *
 * 这一行**不做**任何决定：按了哪颗按钮一律往上抛，真正的动作与两步确认
 * 都在 `useFileList.ts` 那边（删除要点两下才真删，这里只抛第一次）。
 */
import { Lock } from "@lucide/vue";
import { formatBytes, formatTime } from "../../ipc/maintenance.ts";
import { fileReferenceOf } from "../../dom/file-links.ts";
import type { FileEntry } from "../../ipc/files.ts";

/** 改名的草稿名：与别处共用一份，所以是个 model 而不是本地的 ref */
const renameText = defineModel<string>("renameText", { required: true });

const props = defineProps<{
  /** 这一行的那个文件 */
  file: FileEntry;
  /** 正在办一件要等后端的事 */
  busy: boolean;
  /** 这一行正处在"改名"形态吗 */
  renaming: boolean;
  /** 这一行正处在"确认删除"形态吗 */
  confirming: boolean;
  /** 这一行正在传新版（那一下要先选文件） */
  updating: boolean;
}>();

const emit = defineEmits<{
  /** 打开这个文件的那一页 */
  (e: "navigate", title: string): void;
  /** 开始改名 */
  (e: "start-rename"): void;
  /** 确认改名 */
  (e: "save-rename"): void;
  /** 改名那半边的"取消" */
  (e: "cancel-rename"): void;
  /** 确认删除那半边的"取消" */
  (e: "cancel-delete"): void;
  /** 复制引用 */
  (e: "copy-reference"): void;
  /** 传新版（选文件那一下在那一边） */
  (e: "update"): void;
  /** 另存为 */
  (e: "save-as"): void;
  /** 按了「删除」—— 第一遍只是问一次 */
  (e: "request-delete"): void;
}>();

/** 不是图片就显示后缀那三个字母，让人一眼认出这是个什么东西 */
function extensionOf(name: string): string {
  return (name.split(".").pop() ?? "?").toUpperCase();
}
</script>

<template>
  <li class="files__item">
    <button
        class="files__thumb"
        type="button"
        :title="`打开「${file.name}」这一页`"
        @click="emit('navigate', file.title)"
    >
      <img
          v-if="file.mime.startsWith('image/') && !file.needs_unlock"
          :src="file.url"
          :alt="file.name"
      />
      <span v-else-if="file.needs_unlock" class="files__lock" title="加密存的：打开这一页解锁">
        <Lock :size="13" :stroke-width="1.9"/>
      </span>
      <span v-else class="files__ext">{{ extensionOf(file.name) }}</span>
    </button>

    <span class="files__body">
      <template v-if="props.renaming">
        <input
            v-model="renameText"
            class="files__rename"
            type="text"
            @keydown.enter.prevent="emit('save-rename')"
            @keydown.esc="emit('cancel-rename')"
        />
      </template>
      <button
          v-else
          type="button"
          class="files__name"
          @click="emit('navigate', file.title)"
      >
        {{ file.name }}
      </button>

      <span class="files__meta">
        第 {{ file.rev }} 版 · {{ formatBytes(file.size) }} · {{ file.mime }} ·
        改于 {{ formatTime(file.modified) }}
      </span>
      <code class="files__ref">{{ fileReferenceOf(file) }}</code>
    </span>

    <span class="files__actions">
      <template v-if="props.renaming">
        <button
            type="button"
            class="files__btn files__btn--go"
            :disabled="props.busy"
            @click="emit('save-rename')"
        >
          确认改名
        </button>
        <button type="button" class="files__btn" @click="emit('cancel-rename')">取消</button>
      </template>
      <template v-else-if="props.confirming">
        <button
            type="button"
            class="files__btn files__btn--danger"
            :disabled="props.busy"
            @click="emit('request-delete')"
        >
          确认删除
        </button>
        <button type="button" class="files__btn" @click="emit('cancel-delete')">取消</button>
      </template>
      <template v-else>
        <button type="button" class="files__btn" @click="emit('copy-reference')">复制引用</button>
        <button type="button" class="files__btn" :disabled="props.busy" @click="emit('update')">
          {{ props.updating ? "更新中…" : "更新" }}
        </button>
        <button type="button" class="files__btn" @click="emit('start-rename')">重命名</button>
        <button type="button" class="files__btn" @click="emit('save-as')">另存为…</button>
        <button
            type="button"
            class="files__btn files__btn--danger"
            @click="emit('request-delete')"
        >
          删除
        </button>
      </template>
    </span>
  </li>
</template>

<style scoped src="./files-row.css"/>