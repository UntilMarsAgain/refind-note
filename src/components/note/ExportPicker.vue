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

<style scoped src="./export-picker.css"/>

<!--
  导出格式的选择：markdown / html / pdf。

  做成一个独立的小浮层而不是"直接导出 markdown、另外两个塞进子菜单"，
  理由是**三种格式摆在一起才看得出区别** —— 尤其要让人知道 PDF 那条
  是交给浏览器打印的（多一步手动确认），而不是悄悄存到某个地方。
  藏进子菜单的话，这句说明就没有地方放了。

  Esc 取消、`@click.self` 点外面取消，都与 `AppMenu` 一致 —— 两个浮层用两套
  规矩会让人生成"这个怎么关"的疑问。
-->

<script setup lang="ts">
import { onBeforeUnmount, ref } from "vue";
import { FileCode, FileText, Printer } from "@lucide/vue";
import {
  EXPORT_FORMAT_EXTENSIONS,
  EXPORT_FORMAT_LABELS,
  type ExportFormat,
} from "../../dom/file-save.ts";

/** 一档格式：名字、图标、一句话说清它的用处 */
const CHOICES: { format: ExportFormat; icon: typeof FileText; hint: string }[] = [
  {
    format: "markdown",
    icon: FileText,
    hint: "笔记里写下的原文。拿到任何编辑器里都读得懂。",
  },
  {
    format: "html",
    icon: FileCode,
    hint: "渲染后的网页。样式在文件里，双击就能看，也能挂到网上。",
  },
  {
    format: "pdf",
    icon: Printer,
    hint: "打开打印面板，在里面选「另存为 PDF」。排版由浏览器负责。",
  },
];

const open = ref(false);
const resolve = ref<((format: ExportFormat | null) => void) | null>(null);

/** 打开选择框；选了走 `resolve(format)`，取消走 `resolve(null)` */
function ask(): Promise<ExportFormat | null> {
  open.value = true;
  return new Promise((answer) => {
    resolve.value = answer;
  });
}

/** 关掉并把结果交出去（`null` = 取消） */
function finish(format: ExportFormat | null) {
  const answer = resolve.value;
  open.value = false;
  resolve.value = null;
  answer?.(format);
}

/**
 * Esc 要在**组件自己的** keydown 里处理。
 *
 * 放在全局监听里的话，输入框里打字也会被 Esc 截停 —— 而输入框里按 Esc
 * 本来不该关掉整个对话框（那是"清空候选"的意思）。
 */
function onKeydown(event: KeyboardEvent) {
  if (event.key === "Escape") {
    event.preventDefault();
    event.stopPropagation();
    finish(null);
  }
}

onBeforeUnmount(() => {
  // 卸载时还开着：把等待者放掉，否则调用方会一直挂着（await 不返回）
  finish(null);
});

defineExpose({ ask });
</script>

<template>
  <Teleport to="body">
    <!-- @keydown 只在浮层自己挂着时才生效：见 onKeydown 那段说明 -->
    <div
        v-if="open"
        class="picker-backdrop"
        @click.self="finish(null)"
        @keydown="onKeydown"
    >
      <section class="picker" role="dialog" aria-modal="true" aria-label="选择导出格式">
        <header class="picker__head">
          <h2 class="picker__title">导出为</h2>
          <button class="picker__close" type="button" aria-label="关闭" @click="finish(null)">
            ✕
          </button>
        </header>

        <div class="picker__choices">
          <button
              v-for="choice in CHOICES"
              :key="choice.format"
              class="picker__choice"
              type="button"
              @click="finish(choice.format)"
          >
            <component :is="choice.icon" class="picker__icon" :size="18" :stroke-width="1.6"/>
            <span class="picker__label">
              {{ EXPORT_FORMAT_LABELS[choice.format] }}
              <span class="picker__ext">{{ EXPORT_FORMAT_EXTENSIONS[choice.format] }}</span>
            </span>
            <span class="picker__hint">{{ choice.hint }}</span>
          </button>
        </div>
      </section>
    </div>
  </Teleport>
</template>