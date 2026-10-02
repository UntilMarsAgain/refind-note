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
import { ref } from "vue";
import { invoke } from "@tauri-apps/api/core";

/**
 * 删除的二次确认页。
 *
 * 删除是**软删**：日志挪进 `trash/`，所以还捞得回来。这句话要写给点按钮的人看 ——
 * 不然他会以为这是"永久删除"，要么不敢点，要么以为点了就没了。
 */
const props = defineProps<{
  title: string;
}>();

const emit = defineEmits<{
  /** 删掉了：上层该换个地方待着 */
  (e: "deleted"): void;
  /** 不删了：回到这篇的阅读页 */
  (e: "cancel"): void;
}>();

const busy = ref(false);
const error = ref("");

/** 顺手整理一遍：回收这一篇留下的内容块（它引用的内容这时才成为孤儿） */
const tidyUp = ref(false);

async function confirm() {
  busy.value = true;
  error.value = "";

  try {
    await invoke("delete_note", { title: props.title });
    if (tidyUp.value) {
      await invoke("gc", { orphanBlobs: true, orphanDrafts: false });
    }
    emit("deleted");
  } catch (reason) {
    error.value = String(reason);
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <div class="delete">
    <h1 class="delete__title">删除「{{ title }}」？</h1>

    <p class="delete__note">
      删除后，该笔记将从列表中移除，但<strong>内容与历史版本均不会被清除</strong>，
      可在回收站中随时还原，还原后与删除前一致。
    </p>

    <label class="delete__tidy">
      <input v-model="tidyUp" type="checkbox"/>
      <span>
        同时执行一次仓库整理
        <span class="delete__tidy-hint">
          本次删除的笔记仍保留在回收站中，其内容不会被回收；此选项仅顺带清理
          此前已无引用的数据。
        </span>
      </span>
    </label>

    <p v-if="error" class="delete__error">{{ error }}</p>

    <div class="delete__actions">
      <button type="button" class="delete__cancel" :disabled="busy" @click="emit('cancel')">
        取消
      </button>
      <button type="button" class="delete__confirm" :disabled="busy" @click="confirm">
        {{ busy ? "正在删…" : "删除" }}
      </button>
    </div>
  </div>
</template>

<style scoped>
.delete__title {
  margin: 28px 0 12px;
  font-size: 22px;
  font-weight: 600;
  overflow-wrap: anywhere;
}

.delete__note {
  margin: 0;
  color: var(--text-dim);
  font-size: 13.5px;
  line-height: 1.7;
}

.delete__tidy {
  display: flex;
  gap: 8px;
  align-items: flex-start;
  margin: 16px 0 0;
  color: var(--text);
  font-size: 13.5px;
  line-height: 1.6;
  cursor: pointer;
}

.delete__tidy input {
  flex: 0 0 auto;
  margin-top: 4px;
  accent-color: var(--accent);
}

.delete__tidy-hint {
  display: block;
  margin-top: 4px;
  color: var(--text-dim);
  font-size: 12.5px;
  line-height: 1.7;
}

.delete__error {
  margin: 12px 0 0;
  padding: 12px 14px;
  border: 1px solid var(--danger);
  border-left-width: 3px;
  border-radius: 8px;
  color: var(--text);
  font-size: 13.5px;
}

.delete__actions {
  display: flex;
  gap: 10px;
  margin: 20px 0 0;
}

.delete__cancel,
.delete__confirm {
  padding: 7px 16px;
  border: 1px solid var(--border);
  border-radius: 7px;
  background: var(--surface);
  color: var(--text);
  font: inherit;
  font-size: 13.5px;
  cursor: pointer;
}

.delete__cancel:hover:not(:disabled) {
  background: var(--hover);
}

/* 破坏性操作给个能认出来的样子：只有它用危险色 */
.delete__confirm {
  border-color: var(--danger);
  color: var(--danger);
}

.delete__confirm:hover:not(:disabled) {
  background: var(--danger);
  color: var(--bg);
}

.delete__cancel:disabled,
.delete__confirm:disabled {
  opacity: 0.6;
  cursor: default;
}
</style>
