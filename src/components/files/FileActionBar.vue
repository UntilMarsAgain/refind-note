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
 * 文件页底部那一排**动作**。
 *
 * 为什么单独一个组件：这一排里混着两种语气 —— "把这一份带走"（另存为、复制引用）
 * 与"改动这一份"（传新版、交给系统打开）；后者里还有一件**会写出明文**的事，
 * 加密存的要两步确认才放行。把这两种语气并排在一处，就没法一眼看出哪一颗按钮
 * 要人当心，所以那一段警告与它的两个按钮留在同一个组件里。
 *
 * 「传新版」也归在这一排的原因：它带的那个存储选择器说的是"**这一版**怎么存"，
 * 与这排按钮里紧挨着的那颗是同一件事（改内容），所以挨着摆。
 *
 * 底下发生的事在 `composables/useFileDetail.ts`，与文件列表页共用的那几件动作
 * 在 `composables/useFileActions.ts`。
 */
import { Download, ExternalLink, Upload } from "@lucide/vue";
import StoragePicker from "../common/StoragePicker.vue";
import type { FileInfo } from "../../ipc/files.ts";
import type { Policy } from "../../ipc/note.ts";

/**
 * 「传新版」这一版怎么存 —— 与上传页、编辑器同一个选择器，所以是两个 model
 * （策略是存进仓库的，从这一版起粘住；口令只活在这次会话里）。
 */
const policy = defineModel<Policy>("policy", { required: true });
const passphrase = defineModel<string>("passphrase", { required: true });

const props = defineProps<{
  /** 这一份的现状 */
  entry: FileInfo;
  /** 正在等后端 */
  busy: boolean;
  /** "用系统应用打开"的明文提示已经摆出来了吗（加密存的两步确认） */
  confirmingOpen: boolean;
}>();

const emit = defineEmits<{
  /** 传新版：选一个文件，内容替换进来 */
  (e: "update"): void;
  /** 交给系统的默认应用打开（还没确认过加密那件事的话，这一下只是先问一句） */
  (e: "open-system"): void;
  (e: "cancel-open"): void;
  (e: "save-as"): void;
  (e: "copy-reference"): void;
}>();
</script>

<template>
  <div class="file__actions">
    <button type="button" class="file__btn" :disabled="props.busy" @click="emit('update')">
      <Upload :size="14" :stroke-width="1.9"/>
      传新版
    </button>
    <template v-if="props.confirmingOpen">
      <span class="file__warn">
        「{{ entry.name }}」在仓库里是<strong>加密存的</strong>：打开会先解开，
        把一份<strong>明文</strong>写到临时目录再交给外部程序，那一份不受本程序保护。
      </span>
      <button type="button" class="file__btn file__btn--go" :disabled="props.busy" @click="emit('open-system')">
        确认打开
      </button>
      <button type="button" class="file__btn" @click="emit('cancel-open')">取消</button>
    </template>
    <button v-else type="button" class="file__btn" :disabled="props.busy" @click="emit('open-system')">
      <ExternalLink :size="14" :stroke-width="1.9"/>
      用系统应用打开
    </button>
    <button type="button" class="file__btn" @click="emit('save-as')">
      <Download :size="14" :stroke-width="1.9"/>
      另存为…
    </button>
    <button type="button" class="file__btn" @click="emit('copy-reference')">
      复制引用
    </button>

    <!-- 传新版也要能指定怎么存：与上传页、编辑器同一个选择器 -->
    <StoragePicker v-model:policy="policy" v-model:passphrase="passphrase"/>
  </div>
</template>

<style scoped src="./file-buttons.css"/>

<style scoped>
.file__actions {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin: 18px 0 0;
}

/* "会写出明文副本"那一句：说清楚再放行 */
.file__warn {
  flex: 1 1 260px;
  min-width: 0;
  color: var(--text-dim);
  font-size: 12.5px;
  line-height: 1.7;
}

.file__warn strong {
  color: var(--text);
}
</style>