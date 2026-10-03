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
import ProtectionReport from "./ProtectionReport.vue";
import type { Protection } from "../../ipc/note.ts";
import { useProtectionLayers } from "../../composables/useProtectionLayers.ts";
import { useProtectionReport } from "../../composables/useProtectionReport.ts";

/**
 * 这一页在磁盘上是怎么存的。
 *
 * 徽标只说"有什么层"（已压缩 / 已签名 / 已加密 / 口令加密），一句话一枚；
 * 细节（谁签的、验过没有、本机解不解得开）**点开才问后端** —— 那是要花时间的活，
 * 不该在列一张历史清单时替每一行都做一遍。
 *
 * 于是这个组件拆成三处：
 * - **该显示哪几枚**在 `composables/useProtectionLayers.ts`（`Protection` → 文案与
 *   颜色 class 的映射表就写在那儿）；本组件是它唯一的消费者。
 * - **点开才问的那些细节**在 `composables/useProtectionReport.ts`。
 * - **细节卡本身**是子组件 `ProtectionReport.vue`（样式 `protection-report.css`），
 *   徽章那一排的样式是 `storage-badge.css`。
 *
 * 值全部来自 blob 的**明文头**，所以不解锁就能显示 —— "这一页是加密的"这件事
 * 不该等人输了口令才知道。
 */
const props = defineProps<{
  protection: Protection;
  /** 哪一篇：给了才问得动后端那些细节 */
  title?: string;
  /** 哪一版（地址里的版本 token）；不给或 null = 最新版 */
  reference?: string | null;
}>();

/** 哪一枚徽章点开了；一次只开一个 */
const openedKey = ref<string | null>(null);
/** 根元素：给"点到别处就收起"用（挂在模板上那一处 `ref`） */
const rootEl = ref<HTMLElement | null>(null);

const { stored, rows, holdingPassphrase, toggle, forget } = useProtectionReport({
  title: () => props.title,
  reference: () => props.reference,
  symmetric: () => props.protection.symmetric,
  openedKey,
  rootEl,
});

/** 口令暂存的状态由上面那个问出来 —— `symmetric` 那一枚的文案要看它 */
const { layers, label } = useProtectionLayers(() => props.protection, stored);
</script>

<template>
  <span ref="rootEl" class="storage" :title="`存储方式：${label}`">
    <span v-if="layers.length === 0" class="storage__badge">原样</span>

    <template v-for="layer in layers" :key="layer.key">
      <!-- 用 span 装的按钮：这些徽章会出现在**历史列表的行按钮里面**，
           里面再套一个 <button> 是非法结构，点击也会一并触发行本身 -->
      <span
          v-if="layer.reportable && title"
          class="storage__badge storage__badge--ask"
          :class="`storage__badge--${layer.key}`"
          role="button"
          tabindex="0"
          :aria-expanded="openedKey === layer.key"
          title="点开看这一层的细节"
          @click.stop="toggle(layer.key)"
          @keydown.enter.stop.prevent="toggle(layer.key)"
          @keydown.space.stop.prevent="toggle(layer.key)"
      >
        {{ layer.label }}
      </span>

      <span
          v-else
          class="storage__badge"
          :class="`storage__badge--${layer.key}`"
      >
        {{ layer.label }}
      </span>
    </template>

    <ProtectionReport
        v-if="openedKey"
        :rows="rows"
        :holding-passphrase="holdingPassphrase"
        :opened-key="openedKey"
        @forget="forget"
    />
  </span>
</template>

<style scoped src="./storage-badge.css"/>
