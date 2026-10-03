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
 * 加密附件的**原位解锁框**。
 *
 * 为什么单独一个组件：读不动的附件，"不能看"这件事本身就是这一页此刻唯一的内容 ——
 * 它与预览（读得动才摆）、与动作条（读得动才办）说的是两件事，摆在页面里会让
 * 「这一页眼下处在哪个状态」散成三处。收在一个组件里，"读不动"就只有这一种样子。
 *
 * **原位输口令**，不跳页：解锁之后这一页自己就刷新了 —— 跳去解锁页再跳回来，
 * 人要重新找一次刚才看到哪一步。
 */
const passphrase = defineModel<string>("passphrase", { required: true });

const props = defineProps<{
  /** 这一版需要口令才能读吗（gpg 那一层不用问口令，系统代理自己会问） */
  needsPassphrase: boolean;
  /** 正在等后端 */
  busy: boolean;
}>();

const emit = defineEmits<{
  /** 把口令交给本次会话，然后重读一遍 */
  (e: "unlock"): void;
}>();
</script>

<template>
  <div class="file__locked">
    <p class="file__locked-hint">
      {{ needsPassphrase ? "这一份是加密存的，输入口令后显示。" : "这一份是加密存的，解锁后显示。" }}
    </p>
    <div class="file__locked-row">
      <input
          v-if="props.needsPassphrase"
          v-model="passphrase"
          class="file__locked-input"
          type="password"
          placeholder="口令"
          @keydown.enter.prevent="emit('unlock')"
      />
      <button
          type="button"
          class="file__btn file__btn--go"
          :disabled="props.busy"
          @click="emit('unlock')"
      >
        解锁并显示
      </button>
    </div>
  </div>
</template>

<style scoped src="./file-buttons.css"/>

<style scoped>
.file__locked {
  margin: 16px 0 0;
  padding: 12px 14px;
  border: 1px solid var(--border);
  border-radius: 8px;
  background: var(--surface);
}

.file__locked-hint {
  margin: 0 0 10px;
  color: var(--text-dim);
  font-size: 13px;
}

.file__locked-row {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.file__locked-input {
  width: 14em;
  padding: 5px 10px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: var(--field-bg);
  color: var(--text);
  font: inherit;
  font-size: 13px;
}

.file__locked-input:focus {
  outline: none;
  border-color: var(--accent-soft);
}
</style>