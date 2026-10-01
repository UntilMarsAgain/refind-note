<script setup lang="ts">
import './css/clean.css'
import './css/theme.css'
import { computed, onBeforeUnmount, onMounted } from "vue";
import WindowTitleBar from "./components/WindowTitleBar.vue";
import TabRail from "./components/TabRail.vue";
import RenderPane from "./components/RenderPane.vue";
import ContextMenu from "./components/ContextMenu.vue";
import { useTabs } from "./tabs.ts";

/**
 * 窗口骨架。
 *
 * - `WindowTitleBar` 绘制标题栏，与「当前标签页的地址」双向绑定；
 * - `TabRail` 绘制垂直标签页；
 * - `RenderPane` 显示当前标签页（暂时是内部数据，用来验证导航）。
 *
 * 三块之间没有"谁驱动谁"的关系 —— 它们只是**读同一份标签页状态**。
 * 状态本身在 `useTabs()` 里，这里只做组装、导航入口与键盘快捷键。
 * （启动三态、顶栏菜单、真正的视图分发都还没接。）
 */
const {
  tabs,
  activeIndex,
  active,
  committed,
  shakeTick,
  canGoBack,
  canGoForward,
  canReopen,
  select,
  newTab,
  close,
  closeOthers,
  reopenClosed,
  move,
  navigate,
  goBack,
  goForward,
} = useTabs();

/**
 * 标题栏绑定的就是当前标签页的地址字段。
 *
 * 用 get/set 而不是直接绑 `active.address`：当前标签页理论上可能为空。
 * 注意它绑的是**地址栏里的字**（可能在编辑中），
 * 渲染区与标签名一律不看它 —— 那些只认"权威副本"（历史游标那一条）。
 */
const address = computed({
  get: () => active.value?.address ?? "",
  set: (value: string) => {
    if (active.value) {
      active.value.address = value;
    }
  },
});

/** 地址栏回车 = 导航。语法一个字都不在这里解析（见 tabs.ts 里的临时替身） */
function onSubmit(value: string) {
  // 和点链接一样算一次跳转：往历史里推一条，回退键能退回上一条
  void navigate(value, "push");
}

/** 标签栏底下的入口：它们也是地址（`special:xxx`），算一次跳转 */
function openSpecial(page: string) {
  void navigate(`special:${page}`, "push");
}

/** 浏览器习惯的快捷键 */
function onKeydown(event: KeyboardEvent) {
  if (!(event.ctrlKey || event.metaKey)) {
    return;
  }
  const key = event.key.toLowerCase();

  // Ctrl+Shift+T 要先判：否则会被下面的 Ctrl+T 吃掉
  if (key === "t" && event.shiftKey) {
    event.preventDefault();
    reopenClosed();
    return;
  }
  if (key === "t") {
    event.preventDefault();
    newTab();
    return;
  }
  if (key === "w") {
    event.preventDefault();
    close(activeIndex.value);
  }
}

onMounted(() => window.addEventListener("keydown", onKeydown));
onBeforeUnmount(() => window.removeEventListener("keydown", onKeydown));
</script>

<template>
  <div class="app">
    <WindowTitleBar
        v-model="address"
        :committed="committed"
        :can-back="canGoBack"
        :can-forward="canGoForward"
        @back="goBack"
        @forward="goForward"
        @submit="onSubmit"
    />

    <div class="app__main">
      <TabRail
          :tabs="tabs"
          :active="activeIndex"
          :shake-tick="shakeTick"
          :can-reopen="canReopen"
          @select="select"
          @close="close"
          @close-others="closeOthers"
          @reopen="reopenClosed"
          @new-tab="newTab"
          @move="move"
          @settings="openSpecial('settings')"
          @trash="openSpecial('trash')"
      />

      <RenderPane :tabs="tabs" :active="activeIndex"/>
    </div>
  </div>

  <ContextMenu/>
</template>

<style scoped>
.app {
  display: flex;
  flex-direction: column;
  height: 100vh;
}

/*
 * min-height: 0 是必须的：否则这个 flex 项会被内容撑开，
 * 里面渲染区的 overflow 就再也滚不动了。
 */
.app__main {
  display: flex;
  flex: 1 1 auto;
  min-height: 0;
}
</style>
