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
 * 文件（`special:files`）。
 *
 * 文件就是 `File:` 命名空间里的页面（`File:桥.png`）：传一次是新的一版，
 * **同一个名字再传就是更新**，旧版留在历史里；删除进回收站。所以这一页只管
 * 上传、更新、改名、另存为与删除 —— 版本与删除后的那些事，走的是与笔记同一条路。
 *
 * 这一页只留**摆出来的东西**：标题栏（计数、怎么存、上传）、那段说明、以及那一列
 * （每一行是 `FileListRow.vue`）。背后发生的事（读列表、粘贴收文件、两步确认的
 * 改名与删除）在 `useFileList.ts`；与单个文件页共用的那几件动作
 * （传新版、改名、复制引用、另存为）在 `useFileActions.ts`，"怎么存"那一栏在
 * `useFileStorage.ts`。
 *
 * 「怎么存」与「上传」挨着摆（不是巧合）：它们说的是同一件事 —— 接下来传的东西
 * 会怎么存进仓库。
 */
import { Upload } from "@lucide/vue";
import StoragePicker from "../common/StoragePicker.vue";
import FileListRow from "../files/FileListRow.vue";
import { useFileList } from "../../composables/useFileList.ts";

const emit = defineEmits<{
  /** 去别的页面（打开某个文件页面） */
  (e: "navigate", input: string): void;
}>();

/**
 * 这一列背后的东西：状态 + 动作。
 *
 * 逐个摊开而不是留一个 `list` 对象：`<script setup>` 的模板里只有**顶层**的 ref
 * 才会自动解包，摊开了模板才读得出"这一列眼下是什么状态"。
 */
const list = useFileList();

const {
  files,
  loading,
  busy,
  problem,
  renaming,
  renameText,
  confirming,
  updating,
  storage,
  pick,
  update,
  startRename,
  saveRename,
  remove,
  copyReference,
  saveAs,
} = list;

/** 改名那半边的「取消」：这一行回到正常那排按钮（删除的确认是另一回事，不跟着清） */
function cancelRename() {
  renaming.value = "";
}

/** 确认删除那半边的「取消」 */
function cancelDelete() {
  confirming.value = "";
}
</script>

<template>
  <section class="files">
    <div class="files__head">
      <h1 class="files__title">文件</h1>
      <span class="files__count">{{ files.length }} 个</span>
      <StoragePicker
          class="files__storage"
          v-model:policy="storage.policy.value"
          v-model:passphrase="storage.passphrase.value"
      />
      <button class="files__upload" type="button" :disabled="busy" @click="pick">
        <Upload :size="14" :stroke-width="2"/>
        {{ busy ? "上传中…" : "上传文件" }}
      </button>
    </div>

    <p class="files__lead">
      文件保存在仓库里，与笔记同一条路：<strong>同一个名字再传一次就是更新</strong>，
      旧版留在历史里；删除进回收站。上传时可以指定<strong>怎么存</strong>（压缩、签名、
      加密、口令），加密的那些会先摆一个解锁按钮。笔记里按<strong>名称</strong>引用：
      <code>![名称](名称)</code>，或使用图片排版语法
      <code>::image src=名称 align=right width=320</code>。也可以在此页直接按
      <strong>Ctrl+V</strong> 粘贴上传。
    </p>

    <p v-if="problem" class="files__problem">{{ problem }}</p>
    <p v-if="loading" class="files__hint">正在读…</p>
    <p v-else-if="files.length === 0" class="files__hint">
      暂无文件。点击「上传文件」选择，或直接粘贴。
    </p>

    <ol v-else class="files__list">
      <FileListRow
          v-for="file in files"
          :key="file.title"
          :file="file"
          :busy="busy"
          :renaming="renaming === file.title"
          :confirming="confirming === file.title"
          :updating="updating === file.title"
          v-model:rename-text="renameText"
          @navigate="emit('navigate', $event)"
          @start-rename="startRename(file)"
          @save-rename="saveRename(file)"
          @cancel-rename="cancelRename"
          @cancel-delete="cancelDelete"
          @copy-reference="copyReference(file)"
          @update="update(file)"
          @save-as="saveAs(file)"
          @request-delete="remove(file)"
      />
    </ol>
  </section>
</template>

<style scoped>
.files {
  padding-top: 18px;
}

.files__head {
  display: flex;
  flex-wrap: wrap;
  gap: 12px;
  align-items: baseline;
}

.files__title {
  margin: 0;
  font-size: 1.7em;
}

.files__count {
  color: var(--text-dim);
  font-size: 13px;
}

/* 存储方式与上传按钮挨在一起：它们说的是同一件事（这次上传怎么存） */
.files__storage {
  margin-left: auto;
}

.files__upload {
  display: inline-flex;
  gap: 6px;
  align-items: center;
  padding: 6px 12px;
  border: 1px solid var(--accent-soft);
  border-radius: 6px;
  background: transparent;
  color: var(--accent-soft);
  font: inherit;
  font-size: 13px;
  cursor: pointer;
}

.files__upload:hover:not(:disabled) {
  background: var(--accent);
  color: var(--text);
}

.files__upload:disabled {
  opacity: 0.5;
  cursor: default;
}

.files__lead {
  margin: 10px 0 18px;
  color: var(--text-dim);
  font-size: 0.95em;
  line-height: 1.8;
}

.files__lead code {
  padding: 1px 5px;
  border-radius: 4px;
  background: var(--hover);
  font-family: var(--mono-font);
  font-size: 0.9em;
}

.files__problem {
  margin: 0 0 12px;
  padding: 8px 12px;
  border-left: 3px solid var(--danger);
  color: var(--text);
  font-size: 13px;
}

.files__hint {
  color: var(--text-dim);
  font-size: 13.5px;
}

.files__list {
  margin: 0;
  padding: 0;
  list-style: none;
  border: 1px solid var(--border);
  border-radius: 8px;
  overflow: hidden;
}
</style>