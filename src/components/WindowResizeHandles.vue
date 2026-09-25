<script setup lang="ts">
import { getCurrentWindow } from "@tauri-apps/api/window";

// @tauri-apps/api 只在模块内部声明了 ResizeDirection，并没有导出它，
// 所以从 startResizeDragging 的公开签名里推导，避免手抄联合类型产生漂移。
type ResizeDirection = Parameters<
  ReturnType<typeof getCurrentWindow>["startResizeDragging"]
>[0];

/**
 * 无边框窗口的边缘缩放热区。
 *
 * `decorations: false` 会把系统边框（连同边缘拖拽缩放）一起去掉，
 * 所以窗口自己得把缩放补回来，否则窗口大小再也改不了。
 *
 * 需要 core:window:allow-start-resize-dragging 权限。
 * 不想要这套补偿的话，直接删掉本组件及其在 App.vue 中的引用即可。
 */

const appWindow = getCurrentWindow();

const handles: { dir: ResizeDirection; side: string }[] = [
  { dir: "North", side: "n" },
  { dir: "South", side: "s" },
  { dir: "West", side: "w" },
  { dir: "East", side: "e" },
  { dir: "NorthWest", side: "nw" },
  { dir: "NorthEast", side: "ne" },
  { dir: "SouthWest", side: "sw" },
  { dir: "SouthEast", side: "se" },
];
</script>

<template>
  <div class="resize-layer" aria-hidden="true">
    <div
      v-for="h in handles"
      :key="h.dir"
      :class="['resize-handle', `resize-handle--${h.side}`]"
      @mousedown.left.prevent="appWindow.startResizeDragging(h.dir)"
    />
  </div>
</template>

<style scoped>
/* 整层不吃事件，只有具体热区吃 */
.resize-layer {
  position: fixed;
  inset: 0;
  z-index: 50;
  pointer-events: none;
}

.resize-handle {
  position: absolute;
  pointer-events: auto;
}

.resize-handle--n {
  top: 0;
  left: 0;
  /* 避开右侧窗口按钮，免得抢掉它们的点击 */
  right: var(--window-controls-width);
  height: 4px;
  cursor: ns-resize;
}

.resize-handle--s {
  right: 0;
  bottom: 0;
  left: 0;
  height: 4px;
  cursor: ns-resize;
}

.resize-handle--w {
  top: 0;
  bottom: 0;
  left: 0;
  width: 4px;
  cursor: ew-resize;
}

.resize-handle--e {
  top: 0;
  right: 0;
  bottom: 0;
  width: 4px;
  cursor: ew-resize;
}

.resize-handle--nw,
.resize-handle--ne,
.resize-handle--sw,
.resize-handle--se {
  width: 10px;
  height: 10px;
}

.resize-handle--nw {
  top: 0;
  left: 0;
  cursor: nwse-resize;
}

.resize-handle--ne {
  top: 0;
  right: 0;
  cursor: nesw-resize;
}

.resize-handle--sw {
  bottom: 0;
  left: 0;
  cursor: nesw-resize;
}

.resize-handle--se {
  right: 0;
  bottom: 0;
  cursor: nwse-resize;
}
</style>
