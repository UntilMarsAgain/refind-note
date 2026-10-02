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
import { openUrl } from "@tauri-apps/plugin-opener";
import { ExternalLink } from "@lucide/vue";

/**
 * 跨站地址（`zhwiki:条目`）。
 *
 * 这一页不在本仓库 —— 它是别人家的页面，所以打开地址栏里的它就等于**把地址交给浏览器**。
 * 界面这边如实说一句"已经交出去了"，并留一个链接：万一系统没接住（没有默认浏览器、
 * 用户把它关掉了），手动点一下还能再去。
 */
const props = defineProps<{
  /** 显示标题（`zhwiki:条目`） */
  title: string;
  /** 要打开的外部地址 */
  url: string;
}>();

/** 交给系统打开过没有 —— 只做一次，别每次重渲染都把浏览器又叫起来 */
const sent = ref(false);

onMounted(() => {
  if (sent.value) {
    return;
  }
  sent.value = true;
  void openUrl(props.url).catch((error) => console.warn("交给浏览器打开失败：", error));
});
</script>

<template>
  <section class="cross">
    <h1 class="cross__title">{{ title }}</h1>

    <p class="cross__lead">
      这一页不在本仓库，已交给系统浏览器打开。
    </p>

    <a class="cross__link" :href="url" @click.prevent="openUrl(url)">
      <ExternalLink :size="14" :stroke-width="1.9"/>
      {{ url }}
    </a>
  </section>
</template>

<style scoped>
.cross {
  padding-top: 24px;
}

.cross__title {
  margin: 0 0 8px;
  font-size: 1.7em;
  font-weight: 600;
  overflow-wrap: anywhere;
}

.cross__lead {
  margin: 0 0 14px;
  color: var(--text-dim);
  font-size: 13.5px;
  line-height: 1.7;
}

.cross__link {
  display: inline-flex;
  gap: 6px;
  align-items: center;
  padding: 6px 12px;
  border: 1px solid var(--border);
  border-radius: 6px;
  color: var(--link-blue);
  font-size: 13px;
  text-decoration: none;
  overflow-wrap: anywhere;
}

.cross__link:hover {
  border-color: var(--link-blue);
}
</style>
