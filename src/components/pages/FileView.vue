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
 * 文件页面（`File:桥.png`）。
 *
 * 它的正文是**字节**，不是给人读的文本 —— 所以这一页不渲染正文，而是把文件本身
 * 摆出来（能预览的预览，不能的给一句"下载看看"）。能做的事与笔记同源：
 * 看历史、删除（进回收站）、另存为；"改内容"这件事在这里叫**传新版**。
 *
 * 这一页只留**摆出来的东西**：页头、元信息、页内改名、那句"看的是历史里的一版"，
 * 以及三块各管一件事的子组件 —— 解锁框（`FileUnlockBox`）、预览（`FilePreview`）、
 * 动作条（`FileActionBar`）。背后发生的事（读哪一版、能不能读得动、两步确认的
 * "用系统应用打开"、传新版）在 `useFileDetail.ts`；与文件列表页共用的那几件动作
 * 在 `useFileActions.ts`。
 *
 * 为什么这三块是子组件而不是模板片段：它们的样式要跟着自己的元素走
 * （`<style scoped>` 按组件生效），而"读得动 / 读不动"是这一页眼下的**两种**状态 ——
 * 各自成一块之后，"读不动"只有一个样子。
 */
import { toRef } from "vue";
import { formatBytes, formatTime } from "../../ipc/maintenance.ts";
import { fileReferenceOf } from "../../dom/file-links.ts";
import { useFileDetail } from "../../composables/useFileDetail.ts";
import PageHeader from "../note/PageHeader.vue";
import FileUnlockBox from "../files/FileUnlockBox.vue";
import FilePreview from "../files/FilePreview.vue";
import FileRenameBar from "../files/FileRenameBar.vue";
import FileActionBar from "../files/FileActionBar.vue";

const props = defineProps<{
  /** 显示标题（`File:桥.png`） */
  title: string;
  /** 看哪一版（地址里的版本 token）；null 就是最新一版 */
  reference?: string | null;
  /** 正文滚下去了：页头收起 */
  collapsed: boolean;
  /** 这一页星标过没有 */
  starred?: boolean;
}>();

const emit = defineEmits<{
  (e: "navigate", input: string): void;
  (e: "toggle-star"): void;
}>();

/**
 * 看哪一页、往哪儿跳 —— 都由这一页的 props 与事件决定。
 *
 * 下面这些是逐个摊开的，不是留一个 `page` 对象：`<script setup>` 的模板里
 * 只有**顶层**的 ref 才会自动解包，摊开了模板才读得出"这一页眼下是什么状态"。
 */
const detail = useFileDetail({
  title: toRef(props, "title"),
  reference: toRef(props, "reference"),
  navigate: (input) => emit("navigate", input),
});

const {
  entry,
  loading,
  busy,
  problem,
  older,
  readable,
  passphrase,
  renaming,
  renameText,
  confirmingOpen,
  storage,
  actions,
  load,
  onAction,
  unlock,
  update,
  saveAs,
  copyReference,
  saveRename,
  cancelRename,
  openWithSystem,
} = detail;
</script>

<template>
  <div class="file">
    <p v-if="loading" class="file__hint">正在读…</p>

    <div v-else-if="problem" class="file__error">
      <p class="file__error-text">{{ problem }}</p>
      <button type="button" class="file__btn" @click="load">重试</button>
    </div>

    <template v-else-if="entry">
      <PageHeader
          :title="entry.title"
          parent=""
          :collapsed="props.collapsed"
          :actions="actions"
          :starred="props.starred ?? false"
          @action="onAction"
          @toggle-star="emit('toggle-star')"
      />

      <p class="file__meta">
        第 {{ entry.rev }} 版 · {{ formatBytes(entry.size) }} · {{ entry.mime }} ·
        改于 {{ formatTime(entry.modified) }}
      </p>

      <!-- 改名：就在这一页上问（系统那种"打开/保存文件"的窗口拿来做改名答非所问） -->
      <FileRenameBar
          v-if="renaming"
          v-model:text="renameText"
          :busy="busy"
          @save="saveRename"
          @cancel="cancelRename"
      />

      <!-- 看的是历史里的一版：说清楚，并给一条回最新版的路 -->
      <p v-if="older" class="file__older">
        正在看第 {{ entry.rev }} 版，不是最新版。
        <button type="button" class="file__link" @click="emit('navigate', entry.title)">
          回到最新版
        </button>
      </p>

      <!--
        加密的先解锁：**原位输口令**，不跳页 —— 解锁之后这一页自己就刷新了。
        读得动之后：能预览的就地预览，不能预览的给一句实话 + 一个"另存为"。
      -->
      <FileUnlockBox
          v-if="!readable"
          v-model:passphrase="passphrase"
          :needs-passphrase="entry.needs_passphrase"
          :busy="busy"
          @unlock="unlock"
      />

      <FilePreview v-else :entry="entry"/>

      <FileActionBar
          v-model:policy="storage.policy.value"
          v-model:passphrase="storage.passphrase.value"
          :entry="entry"
          :busy="busy"
          :confirming-open="confirmingOpen"
          @update="update"
          @open-system="openWithSystem"
          @cancel-open="confirmingOpen = false"
          @save-as="saveAs"
          @copy-reference="copyReference"
      />

      <code class="file__ref">{{ fileReferenceOf(entry) }}</code>
    </template>
  </div>
</template>

<!-- 按钮那套样式住在 files/ 下，与那几个子组件共用一份；`src` 是相对本文件解析的，
     所以这里要带 `../files/`。 -->
<style scoped src="../files/file-buttons.css"/>

<style scoped>
.file {
  padding-top: 4px;
}

.file__older {
  margin: 8px 0 0;
  color: var(--text-dim);
  font-size: 13px;
}

.file__link {
  padding: 0;
  border: 0;
  background: transparent;
  color: var(--accent-soft);
  font: inherit;
  font-size: 13px;
  cursor: pointer;
}

.file__link:hover {
  text-decoration: underline;
}

.file__meta {
  margin: 10px 0 0;
  color: var(--text-dim);
  font-size: 13px;
}

.file__ref {
  display: inline-block;
  margin: 12px 0 0;
  padding: 2px 8px;
  border-radius: 4px;
  background: var(--hover);
  color: var(--text-dim);
  font-family: var(--mono-font);
  font-size: 12px;
}

.file__hint {
  margin: 28px 0 0;
  color: var(--text-dim);
  font-size: 13.5px;
}

.file__error {
  margin: 28px 0 0;
  padding: 12px 14px;
  border: 1px solid var(--danger);
  border-left-width: 3px;
  border-radius: 8px;
  color: var(--text);
  font-size: 13.5px;
}

.file__error-text {
  margin: 0 0 10px;
}
</style>