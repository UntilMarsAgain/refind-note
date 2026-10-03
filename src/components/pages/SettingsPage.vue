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
import { nextTick, watch } from "vue";
import AppearanceSection from "../settings/AppearanceSection.vue";
import ShortcutSection from "../settings/ShortcutSection.vue";
import WorkspaceSection from "../settings/WorkspaceSection.vue";
import StorageSection from "../settings/StorageSection.vue";
import SyncSection from "../settings/SyncSection.vue";
import { databaseRoot, workspaceRoot } from "../../core/preferences.ts";

/**
 * 设置页（`special:settings`）。
 *
 * 改动即时生效、随后落盘，没有"保存"按钮。
 * 每一行都带一个 id，可以直接当地址锚点：`special:settings#accent` 会跳过来并高亮。
 *
 * 这里只剩两件事：**摆出那几节的顺序**，以及从地址里带章节跳过来时滚到那一项。
 * 各自的东西都在 `components/settings/` 下面：外观与浏览（偏好）、工作目录本身
 * （命名空间 / 历史 / 口令 / 维护）、仓库默认的封装策略、云端同步。
 *
 * 几节之间不套任何一层包裹元素 —— 拆组件只为分文件，页面上仍然是原来那一串平铺的行。
 */
const props = defineProps<{
  /** 地址里的章节：跳到那一项并高亮 */
  focus?: string;
}>();

// 从地址里带章节跳过来时，滚到那一项
watch(
  () => props.focus,
  async (focus) => {
    if (!focus) {
      return;
    }
    await nextTick();
    document.getElementById(focus)?.scrollIntoView({ block: "center" });
  },
  { immediate: true },
);
</script>

<template>
  <section class="settings">
    <h1 class="settings__title">设置</h1>

    <!-- 东西存在哪：只读，给"备份 / 排障"时抄路径用 -->
    <dl class="places">
      <dt>工作目录</dt>
      <dd><code>{{ workspaceRoot || "…" }}</code></dd>
      <dt>数据库</dt>
      <dd><code>{{ databaseRoot || "…" }}</code></dd>
    </dl>
    <p class="settings__where">
      改动即时生效，落在工作目录下的 <code>settings/preferences.json</code>
      （快捷键一节另存 <code>settings/keymap.json</code>）。
    </p>

    <AppearanceSection :focus="focus"/>
    <ShortcutSection :focus="focus"/>
    <WorkspaceSection :focus="focus"/>
    <StorageSection :focus="focus"/>
    <SyncSection :focus="focus"/>
  </section>
</template>

<style scoped>
.settings {
  padding: 28px 0 64px;
}

.settings__title {
  margin: 0 0 10px;
  font-size: 22px;
}

/* 工作目录 / 数据库的位置：只读的两行 */
.places {
  display: grid;
  grid-template-columns: max-content 1fr;
  gap: 2px 12px;
  margin-bottom: 8px;
  font-size: 13px;
}

.places dt {
  color: var(--text-dim);
}

.places dd code {
  padding: 1px 5px;
  border-radius: 4px;
  background: var(--hover);
  font-family: var(--mono-font);
  font-size: 12px;
  overflow-wrap: anywhere;
  /* 路径是要抄下来贴进别处的 */
  -webkit-user-select: text;
  user-select: text;
}

.settings__where {
  margin: 0 0 24px;
  color: var(--text-dim);
  font-size: 13px;
  overflow-wrap: anywhere;
}

.settings__where code {
  padding: 1px 5px;
  border-radius: 4px;
  background: var(--hover);
  font-family: var(--mono-font);
  font-size: 12px;
}
</style>
