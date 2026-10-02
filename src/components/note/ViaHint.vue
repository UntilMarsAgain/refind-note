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
import type { Via } from "../../ipc/address.ts";

/**
 * 「从哪儿来」：这一页是被**哪条指令**带过来的。
 *
 * 有它才说得通"我明明点的是甲，怎么到了乙" —— 与 MediaWiki 标题下面那行
 * "(重定向自 CommonMark)" 同理。随机跳转不写具体名字（每次都不一样，写了反而误导）。
 *
 * 点那个来源去看**它本身**（`@no-command`）：不然一点又被送回来，来回打转。
 */
defineProps<{
  via: Via | null;
}>();

const emit = defineEmits<{
  (e: "open", input: string): void;
}>();
</script>

<template>
  <p v-if="via" class="via">
    <template v-if="via.random">来自随机跳转</template>
    <template v-else>
      重定向自
      <button
          type="button"
          class="via__source"
          :title="`打开「${via.from}」本身（不执行它的指令）`"
          @click="emit('open', `${via.from}@no-command`)"
      >
        {{ via.from }}
      </button>
    </template>
  </p>
</template>

<style scoped>
.via {
  display: flex;
  flex-wrap: wrap;
  gap: 0 4px;
  align-items: baseline;
  margin: 2px 0 0;
  color: var(--text-dim);
  font-size: 12.5px;
  line-height: 1.6;
}

.via__source {
  padding: 0;
  border: 0;
  background: transparent;
  color: var(--link-blue);
  font: inherit;
  font-size: inherit;
  cursor: pointer;
}

.via__source:hover {
  text-decoration: underline;
}
</style>
