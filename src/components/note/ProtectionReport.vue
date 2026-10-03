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
import type { ReportRow } from "../../composables/useProtectionReport.ts";

/**
 * 存储徽标点开后展开的那张**细节卡**（`StorageBadge.vue` 的子组件）。
 *
 * 为什么单独拆：它与徽标那一排是两回事 ——
 * 徽标说的是"有什么层"（不解锁就能显示），这张卡说的是"这一层到底怎么样"
 * （要跑 gpg，点开才问）。两者的样式也各是一套：卡片绝对定位、压在正文上、
 * 有阴影；徽标是一排小圆角标签。把卡片拆出来之后，"它挂在 `.storage` 底下"这件事
 * 只需要在 `StorageBadge.vue` 的模板里看一处。
 *
 * 它**不取任何数据**：`rows` 是问完之后的结果，谁问的、什么时候问，都不归这里 ——
 * 全在 `composables/useProtectionReport.ts` 里。这里只摆行、外加两个出口
 * （"忘掉口令"与那句"系统密钥代理保管"）。
 */
defineProps<{
  /** 那几行：左边是名，右边是值。空数组就是"还没有可说的" */
  rows: ReportRow[];
  /** 这一版有没有留着一个"忘掉口令"的出口（`symmetric` 一层且口令已输入） */
  holdingPassphrase: boolean;
  /** 点开的是哪一层：加密那一层要额外说明口令由系统保管 */
  openedKey: string | null;
}>();

const emit = defineEmits<{
  /** 忘掉这一篇的口令 */
  (e: "forget"): void;
}>();
</script>

<template>
  <!-- 点开才展开的细节。它挂在这行徽章底下，不挡正文 -->
  <span class="report" @click.stop>
    <span v-for="row in rows" :key="row.label" class="report__row">
      <span class="report__label">{{ row.label }}</span>
      <span class="report__value">{{ row.value }}</span>
    </span>

    <!--
      口令只在本次会话的内存里，"不再留着"是一条真实的诉求，所以给一个出口；
      gpg 那几层的口令由系统代理保管（见下面那句），程序碰不到，也就忘不掉。
    -->
    <span v-if="holdingPassphrase" class="report__action">
      <button type="button" class="report__btn" @click="emit('forget')">
        忘掉口令
      </button>
    </span>
    <span v-if="openedKey === 'encrypt'" class="report__note">
      解密所需的口令由系统密钥代理保管，本程序无法清除。
    </span>
  </span>
</template>

<style scoped src="./protection-report.css"/>
