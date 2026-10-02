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
 * 右键菜单：全局只挂一份。
 *
 * 各处的项由各自给出（现在只有标签栏），这里只负责画出来、并保证关得掉。
 */
import { onBeforeUnmount, watch } from "vue";
import { closeMenu, contextMenu } from "../../dom/context-menu.ts";

function onKey(event: KeyboardEvent) {
  if (event.key === "Escape") {
    closeMenu();
  }
}

function onAnyClick() {
  closeMenu();
}

watch(contextMenu, (value) => {
  if (value) {
    window.addEventListener("keydown", onKey);
    // 下一帧再挂点击关闭，否则"打开菜单"的那一次点击会立刻把它关掉
    window.setTimeout(() => window.addEventListener("mousedown", onAnyClick), 0);
  } else {
    window.removeEventListener("keydown", onKey);
    window.removeEventListener("mousedown", onAnyClick);
  }
});

onBeforeUnmount(() => {
  window.removeEventListener("keydown", onKey);
  window.removeEventListener("mousedown", onAnyClick);
});

async function run(item: { run: () => void | Promise<void> }) {
  closeMenu();
  await item.run();
}
</script>

<template>
  <div
    v-if="contextMenu"
    class="menu"
    :style="{ left: contextMenu.x + 'px', top: contextMenu.y + 'px' }"
    @contextmenu.prevent
  >
    <button
      v-for="item in contextMenu.items"
      :key="item.label"
      class="menu__item"
      :class="{ 'menu__item--danger': item.danger }"
      type="button"
      @click="run(item)"
    >
      {{ item.label }}
    </button>
  </div>
</template>

<style scoped>
/* 不透明的底：右键菜单压在内容上，半透明会让下面的字透上来 */
.menu {
  position: fixed;
  z-index: 70;
  display: flex;
  flex-direction: column;
  min-width: 208px;
  padding: 6px;
  border: 1px solid var(--border);
  border-radius: 8px;
  background: var(--surface);
  box-shadow: 0 8px 24px rgb(0 0 0 / 0.18);
}

.menu__item {
  display: block;
  width: 100%;
  padding: 7px 10px;
  border: 0;
  border-radius: 6px;
  background: none;
  color: var(--text);
  font-size: 0.92em;
  text-align: left;
  cursor: pointer;
}

.menu__item:hover {
  background: var(--accent-tint);
}

.menu__item--danger {
  color: var(--danger);
}
</style>
