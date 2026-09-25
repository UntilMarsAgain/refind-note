<script setup lang="ts">
import { onBeforeUnmount, ref, watch } from "vue";
import {
  PanelLeftClose,
  PanelLeftOpen,
  Plus,
  Settings,
  X,
} from "@lucide/vue";
import { PREFERENCE_KEYS, readFlag, writeFlag } from "../settings";

/**
 * 垂直标签栏：只列**打开着的**标签页。
 *
 * 以前这里列仓库里的全部笔记（那是「浏览全部文档」，暂时不做 —— 入口改为
 * `special:newtab`）。现在每个标签页背后就是一个**地址**，点它就是切过去并重新解析。
 *
 * 最顶部是展开 / 收起与新建标签页：收起时只留一个首字（没有图标可用时至少还能分辨），
 * 展开状态记进界面偏好，下次打开保持原样。
 */
const props = defineProps<{
  tabs: { address: string; title: string }[];
  active: number;
  /**
   * 抖动信号：每变一次就让标签抖一下。
   *
   * 用在"关掉最后一个标签、于是又新建了一个"这种场合 —— 抖一下告诉用户
   * 关闭生效了、只是又开了一个，而不是点了没反应。
   */
  shakeTick: number;
}>();

const emit = defineEmits<{
  (e: "select", index: number): void;
  (e: "close", index: number): void;
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

function onDragStart(index: number) {
  dragging.value = index;
}

function onDragOver(index: number) {
  overIndex.value = index;
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

/** 默认收起；展开状态记进界面偏好，下次打开保持原样 */
const collapsed = ref(readFlag(PREFERENCE_KEYS.railCollapsed, true));

function toggle() {
  collapsed.value = !collapsed.value;
  writeFlag(PREFERENCE_KEYS.railCollapsed, collapsed.value);
}

/** 收起时显示的首字。用 Array.from 取，免得多字节/代理对只取到半个。 */
function initialOf(tab: { address: string; title: string }) {
  const source = tab.title || tab.address;
  return Array.from(source)[0] ?? "•";
}
</script>

<template>
  <aside class="rail" :class="{ 'rail--collapsed': collapsed }">
    <div class="rail__head">
      <button
        class="rail__icon"
        type="button"
        :title="collapsed ? '展开标签栏' : '收起标签栏'"
        :aria-label="collapsed ? '展开标签栏' : '收起标签栏'"
        :aria-expanded="!collapsed"
        @click="toggle"
      >
        <component
          :is="collapsed ? PanelLeftOpen : PanelLeftClose"
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
      <li
        v-for="(tab, index) in tabs"
        :key="`${index}-${tab.address}`"
        draggable="true"
        @dragstart="onDragStart(index)"
        @dragover.prevent="onDragOver(index)"
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
            :title="tab.address"
            @click="emit('select', index)"
          >
            <span class="rail__initial">{{ initialOf(tab) }}</span>
            <span class="rail__text">{{ tab.title || tab.address }}</span>
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
    <button
      class="rail__settings"
      type="button"
      aria-label="设置"
      @click="emit('settings')"
    >
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
  width: 168px;
  padding: 10px 8px;
  border-right: 1px solid var(--divider);
  /* 只要上下滚动条：overflow-y 一旦不是 visible，x 轴就会被算成 auto，
     于是会出现多余的横向滚动条。这里显式关掉 x 轴。 */
  overflow-x: hidden;
  overflow-y: auto;
}

.rail--collapsed {
  width: 46px;
}

/* 收起 / 展开时宽度平滑变化 */
.rail {
  transition: width 160ms ease;
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

.rail__list {
  position: relative;
  margin: 0;
  padding: 0;
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

/* 尊重系统的「减少动态效果」 */
@media (prefers-reduced-motion: reduce) {
  .rail,
  .rail__item,
  .tab-move,
  .tab-enter-active,
  .tab-leave-active {
    transition: none;
  }
}
/* 设置入口：贴在列表底下，视觉上与标签错开 */
.rail__settings {
  display: flex;
  flex-shrink: 0;
  align-items: center;
  gap: 8px;
  margin: 4px 6px;
  padding: 6px 8px;
  border: 0;
  border-radius: 7px;
  background: transparent;
  color: var(--text-dim);
  font-size: 13px;
  cursor: pointer;
  text-align: left;
}

.rail__settings:hover {
  background: var(--hover);
  color: var(--text);
}

.rail--collapsed .rail__settings {
  justify-content: center;
}

.rail--collapsed .rail__settings .rail__text {
  display: none;
}
/*
 * 设置入口固定在栏底。两条都是必需的：
 *
 * 1. **滚动的必须是列表本身，而不是整条栏**。原来 `.rail` 自己 overflow-y: auto，
 *    按钮在列表之后，于是标签一多就被顶到滚动区外，看起来像"展开后才出现"。
 * 2. **收起时（栏宽 46px，左右各 8px 内边距 → 内容区 30px）按钮的内边距要同步收窄**，
 *    否则内容宽约 32px 超出而被 overflow-x: hidden 裁掉，图标就看不见。
 */
.rail {
  overflow: hidden;
}

.rail__list {
  flex: 1 1 auto;
  min-height: 0;
  overflow-x: hidden;
  overflow-y: auto;
}

.rail__settings {
  margin-top: auto;
}

/*
 * 收起态：图标要居中，且**不许被压缩**。
 *
 * 收起宽 46px、栏左右内边距各 8px → 内容区 30px。上一版给按钮左右外边距各 6px、
 * 内边距各 6px，剩给图标的只有 6px；而 SVG 作为 flex 项默认 `flex-shrink: 1`，
 * 于是被压成一条细缝（看起来"几乎一个像素"）。现在收掉侧边距与内边距，
 * 并把图标设为不可压缩。
 */
.rail--collapsed .rail__settings {
  justify-content: center;
  margin: 4px 2px;
  padding: 6px 0;
}

.rail__settings svg {
  flex-shrink: 0;
}
/* ---------- 抖动：关掉最后一个标签、于是又新建了一个 ---------- */

/*
 * 横向小幅晃动（不旋转、不放大）：要的是"这里有反应"，不是吸引眼球的特效。
 * 时长与 TabRail 里那个 420ms 的定时器一致 —— 到点摘掉类，下次才能再抖。
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

/* 关掉动效的用户：改成一次淡淡的底色提示 —— 信息不能因为"不喜欢动画"而丢掉 */
@media (prefers-reduced-motion: reduce) {
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
