<script setup lang="ts">
import { onBeforeUnmount, ref, watch } from "vue";
import { PanelLeftClose, PanelLeftOpen, Plus, Settings, X } from "@lucide/vue";
import { openMenu } from "../../dom/context-menu.ts";
import { railCollapsed, toggleRail } from "../../core/preferences.ts";
import type { TabState } from "../../core/tabs.ts";
import { initialOf } from "../../core/title.ts";

/**
 * 垂直标签栏。
 */
const props = defineProps<{
  tabs: TabState[];
  active: number;
  /** 有没有"刚关掉、可以重新打开"的标签页（决定右键菜单里给不给那一项） */
  canReopen: boolean;
  /**
   * 抖动信号：每变一次就让标签动一下。
   *
   * 用在"关掉最后一个标签、于是又新建了一个"这种场合 —— 抖一下告诉用户
   * 关闭生效了、只是又开了一个，而不是点了没反应。
   */
  shakeTick: number;
}>();

const emit = defineEmits<{
  (e: "select", index: number): void;
  (e: "close", index: number): void;
  (e: "close-others", index: number): void;
  /** 重新打开刚关掉的那个（右键菜单里给一项入口，快捷键是 Ctrl+Shift+T） */
  (e: "reopen"): void;
  (e: "new-tab"): void;
  /** 拖放调整顺序 */
  (e: "move", from: number, to: number): void;
  (e: "settings"): void;
}>();

/** 正在抖的是哪一格（连索引一起记下来：之后切标签不该把动画挪走） */
const shakeIndex = ref<number | null>(null);
let shakeTimer: number | undefined;

watch(
  () => props.shakeTick,
  () => {
    shakeIndex.value = props.active;
    window.clearTimeout(shakeTimer);
    // 与 CSS 里的动画时长一致；到点把类摘掉，下次才能再触发
    shakeTimer = window.setTimeout(() => {
      shakeIndex.value = null;
    }, 420);
  },
);

onBeforeUnmount(() => window.clearTimeout(shakeTimer));

/** 正在拖的标签页与拖到哪一格上面（用来画插入位置） */
const dragging = ref<number | null>(null);
const overIndex = ref<number | null>(null);

function onDragStart(event: DragEvent, index: number) {
  dragging.value = index;
  // 不带数据的拖动在 WebKit 里根本不算"拖起来"（拖到一半就没有了），
  // 所以哪怕用不上也塞一份进去
  event.dataTransfer?.setData("text/plain", String(index));
  if (event.dataTransfer) {
    event.dataTransfer.effectAllowed = "move";
  }
}

function onDragOver(event: DragEvent, index: number) {
  overIndex.value = index;
  // 光标显示成"移动"，而不是"复制"
  if (event.dataTransfer) {
    event.dataTransfer.dropEffect = "move";
  }
}

function onDragEnd() {
  dragging.value = null;
  overIndex.value = null;
}

function onDrop(index: number) {
  const from = dragging.value;
  dragging.value = null;
  overIndex.value = null;
  if (from !== null && from !== index) {
    emit("move", from, index);
  }
}

/**
 * 标签页上的右键：关闭 / 关闭其它。
 *
 * 中键关闭是浏览器时代的习惯（上面已经有了），右键这两项是给"一口气关掉一堆"用的。
 */
function onTabMenu(event: MouseEvent, index: number) {
  event.preventDefault();
  event.stopPropagation();
  const items = [
    { label: "关闭标签页", run: () => emit("close", index) },
    { label: "关闭其它标签页", run: () => emit("close-others", index) },
  ];
  // 没有关掉过标签页时不给这一项：给了也没得开
  if (props.canReopen) {
    items.push({ label: "重新打开关闭的标签页", run: () => emit("reopen") });
  }
  openMenu(event, items);
}
</script>

<template>
  <aside class="rail" :class="{ 'rail--collapsed': railCollapsed }">
    <div class="rail__head">
      <button
        class="rail__icon rail__icon--fold"
        type="button"
        :title="railCollapsed ? '展开标签栏' : '收起标签栏'"
        :aria-label="railCollapsed ? '展开标签栏' : '收起标签栏'"
        :aria-expanded="!railCollapsed"
        @click="toggleRail"
      >
        <component
          :is="railCollapsed ? PanelLeftOpen : PanelLeftClose"
          :size="14"
          :stroke-width="1.9"
        />
      </button>

      <button
        class="rail__icon"
        type="button"
        title="新建标签页"
        aria-label="新建标签页"
        @click="emit('new-tab')"
      >
        <Plus :size="14" :stroke-width="2" />
      </button>
    </div>

    <TransitionGroup name="tab" tag="ol" class="rail__list">
      <!-- 用标签页自己的 id 作 key，不用下标：重排时下标会变，动画就会错位 -->
      <li
        v-for="(tab, index) in tabs"
        :key="tab.id"
        draggable="true"
        @dragstart="onDragStart($event, index)"
        @dragover.prevent="onDragOver($event, index)"
        @drop.prevent="onDrop(index)"
        @dragend="onDragEnd"
      >
        <div
          class="rail__item"
          :class="{
            'rail__item--active': index === active,
            'rail__item--over': overIndex === index && dragging !== index,
            'rail__item--shake': shakeIndex === index,
          }"
          @mousedown.middle.prevent="emit('close', index)"
        >
          <button
            class="rail__pick"
            type="button"
            :title="tab.address || '新标签页'"
            @click="emit('select', index)"
            @contextmenu="onTabMenu($event, index)"
          >
            <span class="rail__initial">{{ initialOf(tab.title || tab.address) }}</span>
            <span class="rail__text">{{ tab.title || tab.address || "新标签页" }}</span>
          </button>
          <button
            class="rail__close"
            type="button"
            title="关闭"
            aria-label="关闭"
            @click="emit('close', index)"
          >
            <X :size="12" :stroke-width="2" />
          </button>
        </div>
      </li>
    </TransitionGroup>

    <!-- 设置入口：放在标签列表底下，与标签区分开（它不是一个标签） -->
    <button class="rail__entry" type="button" aria-label="设置" @click="emit('settings')">
      <Settings :size="16" :stroke-width="1.75" />
      <span class="rail__text">设置</span>
    </button>
  </aside>
</template>

<style scoped>
.rail {
  display: flex;
  flex: 0 0 auto;
  flex-direction: column;
  gap: 6px;
  width: var(--rail-width);
  padding: 10px 8px;
  border-right: 1px solid var(--divider);
  /* 收起 / 展开时宽度平滑变化 */
  transition: width 160ms ease;
  /* 只要上下滚动条：这里显式关掉两轴，滚动的交给下面的列表 */
  overflow: hidden;
}

.rail--collapsed {
  width: var(--rail-collapsed-width);
}

.rail__head {
  display: flex;
  gap: 6px;
  align-items: center;
}

.rail--collapsed .rail__head {
  flex-direction: column;
}

.rail__icon {
  appearance: none;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 26px;
  height: 26px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: transparent;
  color: var(--text-dim);
  cursor: pointer;
}

.rail__icon:hover {
  background: var(--hover);
  color: var(--text);
}

/*
 * 滚动的必须是列表本身，而不是整条栏：
 * 设置 / 回收站两个入口钉在栏底（margin-top: auto），跟着列表滚就会漂走。
 */
.rail__list {
  position: relative;
  flex: 1 1 auto;
  min-height: 0;
  margin: 0;
  padding: 0;
  overflow-x: hidden;
  overflow-y: auto;
  list-style: none;
}

/* 标签页的出现、消失、重排（重排也走 transform，所以拖动换序是滑过去的） */
.tab-move,
.tab-enter-active,
.tab-leave-active {
  transition:
    transform 160ms ease,
    opacity 160ms ease;
}

.tab-enter-from,
.tab-leave-to {
  opacity: 0;
  transform: translateX(-8px);
}

/* 正在离开的项要脱离文档流，否则剩下的项会先跳一下再滑 */
.tab-leave-active {
  position: absolute;
  width: 100%;
}

/* 拖放时提示插入位置 */
.rail__item--over {
  box-shadow: inset 0 2px 0 var(--accent);
}

.rail__item {
  display: flex;
  align-items: center;
  border-radius: 6px;
  transition:
    background-color 120ms ease,
    box-shadow 120ms ease;
}

.rail__item:hover,
.rail__item--active {
  background: var(--hover);
}

.rail__pick {
  appearance: none;
  display: flex;
  flex: 1 1 auto;
  gap: 8px;
  align-items: center;
  min-width: 0;
  padding: 6px 8px;
  border: 0;
  background: transparent;
  color: var(--text-dim);
  font-size: 13px;
  line-height: 1.6;
  text-align: left;
  cursor: pointer;
}

.rail__item--active .rail__pick {
  color: var(--text);
}

.rail__initial {
  display: none;
  flex: 0 0 auto;
  color: var(--accent-soft);
  font-size: 13px;
}

.rail__text {
  display: block;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.rail--collapsed .rail__initial {
  display: block;
}

.rail--collapsed .rail__text {
  display: none;
}

.rail--collapsed .rail__pick {
  justify-content: center;
  padding: 6px 0;
}

.rail--collapsed .rail__close {
  display: none;
}

.rail__close {
  appearance: none;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 22px;
  height: 22px;
  margin-right: 4px;
  border: 0;
  border-radius: 5px;
  background: transparent;
  color: var(--text-dim);
  cursor: pointer;
  opacity: 0;
}

.rail__item:hover .rail__close {
  opacity: 1;
}

.rail__close:hover {
  background: var(--press);
  color: var(--text);
}

/* 设置 / 回收站入口：贴在列表底下，视觉上与标签错开 */
.rail__entry {
  display: flex;
  flex-shrink: 0;
  align-items: center;
  gap: 8px;
  margin: 4px 6px;
  margin-top: auto;
  padding: 6px 8px;
  border: 0;
  border-radius: 7px;
  background: transparent;
  color: var(--text-dim);
  font-size: 13px;
  cursor: pointer;
  text-align: left;
}

.rail__entry:hover {
  background: var(--hover);
  color: var(--text);
}

/* SVG 作为 flex 项默认会被压扁，收起态的窄栏里必须钉住它 */
.rail__entry svg {
  flex-shrink: 0;
}

/*
 * 收起态：图标要居中，且内外边距都要收窄。
 * 收起宽 46px、栏左右各 8px 内边距 → 内容区 30px；
 * 沿用展开态的内边距会把图标挤出可视范围（被 overflow 裁掉，看起来像消失了）。
 */
.rail--collapsed .rail__entry {
  justify-content: center;
  margin: 4px 2px;
  padding: 6px 0;
}

/* ---------- 抖动：关掉最后一个标签、于是又新建了一个 ---------- */

/*
 * 横向小幅晃动（不旋转、不放大）：要的是"这里有反应"，不是吸引眼球的特效。
 * 时长与脚本里那个 420ms 的定时器一致 —— 到点摘掉类，下次才能再抖。
 */
.rail__item--shake {
  animation: rail-shake 420ms ease-in-out;
}

@keyframes rail-shake {
  0%,
  100% {
    transform: translateX(0);
  }

  20% {
    transform: translateX(-3px);
  }

  40% {
    transform: translateX(3px);
  }

  60% {
    transform: translateX(-2px);
  }

  80% {
    transform: translateX(2px);
  }
}

/* ---------- 窗口窄了：竖排标签栏挪到页面下方，横过来 ---------- */

/*
 * 窄窗口里 168px 的竖栏要占掉四分之一，正文被挤成一条；横过来之后它只吃几十像素高。
 *
 * 断点 760px 与 `App.vue` 里把 `.app__main` 改成竖排的那条是**同一个数** ——
 * 两处一起改，否则会出现"栏横过来了但还在左边"这种半截状态。
 *
 * 横过来之后展开/收起没有意义（省不出空间），所以那枚按钮藏起来，
 * 收起态的样式也一并作废 —— 这样"上次收起着"的人在窄窗口里也看得见标签名。
 */
@media (max-width: 760px) {
  .rail,
  .rail--collapsed {
    /* 排到正文下面（`order` 见 App.vue 里那条竖排规则） */
    order: 2;
    flex-direction: row;
    align-items: center;
    width: 100%;
    height: 44px;
    padding: 4px 6px;
    border-right: 0;
    border-top: 1px solid var(--divider);
  }

  .rail__head,
  .rail--collapsed .rail__head {
    flex-direction: row;
  }

  .rail__icon--fold {
    display: none;
  }

  /* 标签横着排一行，太长就在这一条里横向滚 */
  .rail__list {
    display: flex;
    flex: 1 1 auto;
    gap: 4px;
    align-items: center;
    overflow-x: auto;
    overflow-y: hidden;
  }

  .rail__list > li {
    flex: 0 0 auto;
    max-width: 180px;
  }

  .rail__item,
  .rail--collapsed .rail__item {
    width: auto;
  }

  .rail__item--over {
    /* 横排里插入位置在**左边** */
    box-shadow: inset 2px 0 0 var(--accent);
  }

  .rail__pick,
  .rail--collapsed .rail__pick {
    justify-content: flex-start;
    padding: 4px 8px;
  }

  /* 收起态那套"只留首字"作废：窄窗口里更要知道这一格是哪一页 */
  .rail--collapsed .rail__initial {
    display: none;
  }

  .rail--collapsed .rail__text {
    display: block;
  }

  .rail--collapsed .rail__close,
  .rail__close {
    display: inline-flex;
    /* 横排的格子里没有"悬停才出现"的余地：这么窄的一条，找不着就是找不着 */
    opacity: 0.7;
  }

  .rail__entry,
  .rail--collapsed .rail__entry {
    justify-content: center;
    margin: 0;
    padding: 4px 8px;
  }

  /* 竖向堆叠时"正在离开的项脱离文档流"是为了别让剩下的项跳一下；
     横排里那条绝对定位会盖住整行，改成不脱流 */
  .tab-leave-active {
    position: static;
    width: auto;
  }
}

@media (prefers-reduced-motion: reduce) {
  .rail,
  .rail__item,
  .tab-move,
  .tab-enter-active,
  .tab-leave-active {
    transition: none;
  }

  /* 关掉动效的用户：改成一次淡淡的底色提示 —— 信息不能因为"不喜欢动画"而丢掉 */
  .rail__item--shake {
    animation: rail-flash 420ms ease-out;
  }

  @keyframes rail-flash {
    from {
      background-color: var(--accent-tint);
    }

    to {
      background-color: transparent;
    }
  }
}
</style>
