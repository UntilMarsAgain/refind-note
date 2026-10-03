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
import { onMounted, ref } from "vue";

/**
 * 改名：输入框 + 确认/取消 + 一句提醒，**在页面里**问。
 *
 * 为什么不借系统的文件对话框：系统给应用的那种窗口只有"打开/保存文件"，
 * 拿它当改名的输入框，看上去就像要把文件存到哪儿去，答非所问。
 *
 * 那句提醒跟着输入框一起摆：**笔记里写的是名字，仓库里改的是页面名**，
 * 两者从此各说各的 —— 不说清楚的话，人会以为刚才那些 `![桥](桥.png)` 自己跟过来了。
 */
const text = defineModel<string>("text", { required: true });

const props = defineProps<{
  /** 正在等后端 */
  busy: boolean;
}>();

const emit = defineEmits<{
  (e: "save"): void;
  (e: "cancel"): void;
}>();

const input = ref<HTMLInputElement | null>(null);

/**
 * 摆出来的第一件事：选中原来的名字。
 *
 * 人在**改**这个名字，不是在**重写**它 —— 所以选中而不是清空，光标直接落在末尾。
 * 这个组件是 `v-if` 出来的，挂载即"输入框刚摆出来"，所以选中就在这一刻做；
 * （原先是页面里 `nextTick` 之后拿模板 ref 选，效果一样。）
 */
onMounted(() => input.value?.select());
</script>

<template>
  <div class="file__rename">
    <input
        ref="input"
        v-model="text"
        class="file__rename-input"
        type="text"
        @keydown.enter.prevent="emit('save')"
        @keydown.esc="emit('cancel')"
    />
    <button type="button" class="file__btn file__btn--go" :disabled="props.busy" @click="emit('save')">
      确认改名
    </button>
    <button type="button" class="file__btn" @click="emit('cancel')">取消</button>
    <span class="file__rename-hint">笔记里已写下的旧名称不会随之更改。</span>
  </div>
</template>

<style scoped src="./file-buttons.css"/>

<style scoped>
.file__rename {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  align-items: center;
  margin: 12px 0 0;
  padding: 10px 12px;
  border: 1px solid var(--border);
  border-radius: 8px;
  background: var(--surface);
}

.file__rename-input {
  flex: 1 1 240px;
  min-width: 0;
  padding: 5px 10px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: var(--field-bg);
  color: var(--text);
  font: inherit;
  font-size: 13px;
}

.file__rename-input:focus {
  outline: none;
  border-color: var(--accent-soft);
}

.file__rename-hint {
  color: var(--text-dim);
  font-size: 12px;
}
</style>