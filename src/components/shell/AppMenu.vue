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
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { BookOpen } from "@lucide/vue";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { openMenu } from "../../dom/context-menu.ts";
import type { HelpPage } from "../../ipc/help.ts";
import { FALLBACK_GROUP, metaOf, SPECIAL_GROUPS } from "../../core/special.ts";
import { logoSrc } from "../../core/theme.ts";

/**
 * 顶栏菜单：列出所有特殊页面。
 *
 * 版式是**下拉面板**：贴在标题栏下方、标签栏右侧，顶上一行站点名，下面按分组竖列条目。
 *
 * **条目来自后端**（`special_pages`）：后端说有哪些页面，这里就有哪些；显示名与图标
 * 从 `special.ts` 取，没登记的会以 `special:<名字>` 出现在「其它」里。
 *
 * 页面**不压暗**：菜单是"在页面之上开一张表"，不是模态框。
 */
const props = defineProps<{
  /** 后端给出的、当前存在的特殊页面名 */
  pages: string[];
  /** 帮助页（随程序发布）：单独一组摆在最前面 */
  help: HelpPage[];
  /** 站点名（面板顶上那一行） */
  title: string;
  /**
   * 是否展开。
   *
   * 由父组件持有、这里只负责显示：组件**始终挂载**，出现与收起才能各自播完动画
   * （父组件用 `v-if` 的话，卸载是立刻的，收起动画根本来不及播）。
   */
  open: boolean;
}>();

const emit = defineEmits<{
  (e: "open", address: string): void;
  /** Ctrl/Cmd 点击，或者右键菜单里选「在新标签页打开」 */
  (e: "open-new-tab", address: string): void;
  (e: "close"): void;
}>();

/** 点条目：Ctrl/Cmd 点击＝在新标签页打开（与正文里的链接一个规矩） */
function activate(event: MouseEvent, page: string) {
  if (event.ctrlKey || event.metaKey) {
    emit("open-new-tab", `special:${page}`);
    return;
  }
  emit("open", `special:${page}`);
}

/**
 * 条目上右键：与浏览器一致，给「在新标签页打开」。
 *
 * 顺带一项「复制地址」—— 菜单里都是本程序的系统页面，地址写成 `special:xxx`，
 * 抄下来贴进地址栏就能打开。
 */
function onItemMenu(event: MouseEvent, page: string) {
  event.preventDefault();
  event.stopPropagation();
  const address = `special:${page}`;
  openMenu(event, [
    { label: "在新标签页打开", run: () => emit("open-new-tab", address) },
    { label: "复制地址", run: () => writeText(address) },
  ]);
}

// Esc 关闭：这一版菜单不压暗页面，键盘出口要留一个
function onKeydown(event: KeyboardEvent) {
  if (props.open && event.key === "Escape") {
    emit("close");
  }
}

onMounted(() => window.addEventListener("keydown", onKeydown));
onBeforeUnmount(() => window.removeEventListener("keydown", onKeydown));

/**
 * 面板左侧留出标签栏：菜单不该盖住标签页。
 *
 * 宽度**当场量**而不是写死一个数 —— 标签栏会展开/收起（两档宽度），
 * 写死就会在其中一种状态下盖住它、或离得老远。
 */
const panelLeft = ref(8);

function measureLeft() {
  const rail = document.querySelector(".rail");
  const right = rail?.getBoundingClientRect().right ?? 0;
  panelLeft.value = Math.round(right) + 8;
}

/** 位置与宽度都跟着左边距走，窄窗口下也不会顶出屏幕 */
const panelStyle = computed(() => ({
  left: `${panelLeft.value}px`,
  // 宽一点：分组是**并排的几列**，挤在一起就退化成一张两行的表（不好看也不好扫）。
  // 820 是"四列各自放得下、且不至于把整页盖住"的宽度；窗口窄了就按比例缩，
  // 缩到放不下时列数自然减少（grid 那条 auto-fit 管这个）。
  width: `min(820px, calc(100vw - ${panelLeft.value + 16}px))`,
}));

// 每次展开都重新量一次：展开与收起两档宽度不同，上次的数字不能留用
function onBeforeEnter() {
  measureLeft();
}

/**
 * 打开一条帮助页。
 *
 * 帮助不走 `special:` —— 它是**自己的命名空间**（`Help:首页`），所以这里直拼地址。
 */
function openHelp(event: MouseEvent, page: HelpPage) {
  const address = `Help:${page.slug}`;
  if (event.ctrlKey || event.metaKey) {
    emit("open-new-tab", address);
    return;
  }
  emit("open", address);
}

/** 按分组归拢；顺序由 SPECIAL_GROUPS 决定，空分组不显示 */
const groups = computed(() =>
  [...SPECIAL_GROUPS, FALLBACK_GROUP]
    .filter((group, index, all) => all.indexOf(group) === index)
    .map((group) => ({
      title: group,
      items: props.pages.filter((page) => metaOf(page).group === group),
    }))
    .filter((group) => group.items.length > 0),
);
</script>

<template>
  <!-- 点面板外面收起 -->
  <Transition name="menu" @before-enter="onBeforeEnter">
    <div v-if="open" class="menu-backdrop" @click.self="emit('close')">
      <section class="menu" :style="panelStyle">
        <header class="menu__head">
          <img class="menu__logo" :src="logoSrc" alt="" draggable="false" />
          <span class="menu__title">{{ title }}</span>
        </header>

        <div class="menu__columns">
          <!-- 帮助摆在第一组：它是"怎么用这个程序"，比任何一个系统页面都靠前 -->
          <nav v-if="help.length > 0" class="menu__group">
            <h2 class="menu__group-title">帮助</h2>
            <button
              v-for="page in help"
              :key="page.slug"
              class="menu__item"
              type="button"
              :title="page.display"
              @click="openHelp($event, page)"
            >
              <BookOpen class="menu__icon" :size="18" :stroke-width="1.6" />
              <span class="menu__label">{{ page.slug }}</span>
            </button>
          </nav>

          <nav v-for="group in groups" :key="group.title" class="menu__group">
            <h2 class="menu__group-title">{{ group.title }}</h2>
            <button
              v-for="page in group.items"
              :key="page"
              class="menu__item"
              type="button"
              :title="metaOf(page).tip"
              @click="activate($event, page)"
              @contextmenu="onItemMenu($event, page)"
            >
              <component
                :is="metaOf(page).icon"
                v-if="metaOf(page).icon"
                class="menu__icon"
                :size="18"
                :stroke-width="1.6"
              />
              <span class="menu__label">{{ metaOf(page).label }}</span>
              <!-- 快捷键提示：登记了才显示（现在还没有，见 `special.ts`） -->
              <span v-if="metaOf(page).shortcut?.length" class="menu__keys">
                <kbd v-for="key in metaOf(page).shortcut" :key="key" class="menu__key">
                  {{ key }}
                </kbd>
              </span>
            </button>
          </nav>
        </div>
      </section>
    </div>
  </Transition>
</template>

<style scoped>
.menu-backdrop {
  position: fixed;
  inset: 0;
  z-index: 54;
  /* 只负责"点外面关掉"，背景保持透明，页面照旧可见 */
  background: transparent;
}

/* 出现 / 收起：140ms，菜单不该让人等 */
.menu-enter-active,
.menu-leave-active {
  transition: opacity 140ms ease;
}

.menu-enter-active .menu,
.menu-leave-active .menu {
  transition: transform 140ms ease;
}

.menu-enter-from,
.menu-leave-to {
  opacity: 0;
}

.menu-enter-from .menu,
.menu-leave-to .menu {
  transform: translateY(-8px);
}

/* 关掉动效的用户：直接出现 / 消失，不做位移与淡出 */
@media (prefers-reduced-motion: reduce) {
  .menu-enter-active,
  .menu-leave-active,
  .menu-enter-active .menu,
  .menu-leave-active .menu {
    transition: none;
  }

  .menu-enter-from .menu,
  .menu-leave-to .menu {
    transform: none;
  }
}

.menu {
  position: absolute;
  top: var(--titlebar-height);
  max-height: calc(100vh - var(--titlebar-height) - 12px);
  overflow-y: auto;
  padding: 20px 22px 24px;
  border: 1px solid var(--border);
  border-top: 0;
  border-radius: 0 0 12px 0;
  background: var(--bg);
  box-shadow: 0 22px 48px rgb(0 0 0 / 48%);
}

/* 顶上一行站点名，下面一条通到底的细线，再往下才是那几列 */
.menu__head {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 12px;
  padding-bottom: 16px;
  margin-bottom: 20px;
  border-bottom: 1px solid var(--border);
}

.menu__logo {
  width: 30px;
  height: 30px;
  -webkit-user-drag: none;
}

.menu__title {
  font-size: 22px;
  font-weight: 600;
  letter-spacing: 0.02em;
}

.menu__columns {
  display: grid;
  /* 分组是**并排的列**（不是一张表）：一行摆得下就一行 */
  grid-template-columns: repeat(auto-fit, minmax(168px, 1fr));
  gap: 20px 20px;
  align-items: start;
}

.menu__group-title {
  margin: 0 0 4px;
  /* 与条目的图标对齐：条目自己带 10px 左内边距 */
  padding: 0 10px;
  color: var(--text-dim);
  font-size: 12.5px;
  font-weight: 400;
}

.menu__item {
  display: flex;
  align-items: center;
  gap: 10px;
  width: 100%;
  padding: 9px 10px;
  border: 0;
  border-radius: 8px;
  background-color: transparent;
  color: var(--text);
  font-size: 14.5px;
  text-align: left;
  cursor: pointer;
}

.menu__item:hover {
  background-color: var(--hover);
}

.menu__icon {
  flex: 0 0 auto;
}

.menu__label {
  flex: 1;
  /* 菜单项一律一行：地方不够就挤一挤，宁可缩写也不要竖着排 */
  white-space: nowrap;
}

/* 右侧那排小方键（照参考图）：只有登记了快捷键的条目才有 */
.menu__keys {
  display: flex;
  gap: 4px;
  margin-left: auto;
}

.menu__key {
  min-width: 22px;
  padding: 2px 5px;
  border: 1px solid var(--border);
  border-radius: 5px;
  background: var(--field-bg);
  color: var(--text-dim);
  font-family: inherit;
  font-size: 11px;
  text-align: center;
}
</style>
