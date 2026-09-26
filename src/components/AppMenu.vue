<script setup lang="ts">
/**
 * 顶栏菜单：列出所有特殊页面。
 *
 * 版式参照 MediaWiki 的站点菜单 —— 贴在标题栏下面整幅铺开，顶上一行站点名，
 * 下面按分组竖列条目。**条目来自后端**（`special_pages` 命令）：后端说有哪些页面，
 * 这里就有哪些；显示名与图标从 `special.ts` 取，没登记的会以 `special:<名字>`
 * 出现在「其它」里，不会因为忘了登记而消失。
 */
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { FALLBACK_GROUP, metaOf, SPECIAL_GROUPS } from "../special";
import { logoSrc } from "../theme";

const props = defineProps<{
  /** 后端给出的、当前存在的特殊页面名 */
  pages: string[];
  /** 站点名（顶栏那一行） */
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
  (e: "close"): void;
}>();

// Esc 关闭：这一版菜单不压暗页面，键盘出口要留一个
function onKeydown(event: KeyboardEvent) {
  if (props.open && event.key === "Escape") {
    emit("close");
  }
}

/**
 * 面板左侧留出标签栏：菜单不该盖住标签页。
 *
 * 宽度**当场量**而不是写死一个数 —— 标签栏会展开/收起（168px / 46px），
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
  width: `min(520px, calc(100vw - ${panelLeft.value + 16}px))`,
}));

watch(
  () => props.open,
  (open) => {
    if (open) {
      measureLeft();
    }
  },
);

onMounted(() => window.addEventListener("keydown", onKeydown));
onBeforeUnmount(() => window.removeEventListener("keydown", onKeydown));

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
  <!--
    点面板外面收起；面板贴在标题栏下方、**标签栏右侧**（不盖住标签页）。
    出现与收起都有动画：整块淡入淡出，面板自上而下落一点。
  -->
  <Transition name="menu">
    <div v-if="open" class="menu-backdrop" @click.self="emit('close')">
      <section class="menu" :style="panelStyle">
        <header class="menu__head">
          <img class="menu__logo" :src="logoSrc" alt="" />
          <h1 class="menu__title">{{ title }}</h1>
        </header>

        <div class="menu__columns">
          <nav v-for="group in groups" :key="group.title" class="menu__group">
            <h2 class="menu__group-title">{{ group.title }}</h2>
            <button
              v-for="page in group.items"
              :key="page"
              class="menu__item"
              type="button"
              :title="metaOf(page).tip"
              @click="emit('open', `special:${page}`)"
            >
              <component
                :is="metaOf(page).icon"
                v-if="metaOf(page).icon"
                :size="16"
                :stroke-width="1.75"
              />
              <span>{{ metaOf(page).label }}</span>
            </button>
            </nav>
          </div>
      </section>
    </div>
  </Transition>
</template>

<style scoped>
/*
 * 版式对照参考图：**下拉面板**，不是整幅铺开。
 *
 * - 贴着菜单键正下方、左边齐平；上边不圆角（接着标题栏），下边圆角；
 * - 页面**不压暗**：菜单是"在页面之上开一张表"，不是模态框；
 * - 内容多了在面板内滚动，不把整页撑长。
 */
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

/* 关掉动效的用户：直接出现/消失，不做位移与淡出 */
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
  padding: 14px 20px 20px;
  border: 1px solid var(--border);
  border-top: 0;
  border-radius: 0 0 10px 0;
  background: var(--bg);
  box-shadow: 0 18px 40px rgb(0 0 0 / 45%);
}

.menu__head {
  display: flex;
  align-items: center;
  /* 参考图里站点名是居中的 */
  justify-content: center;
  gap: 10px;
  margin-bottom: 16px;
}

.menu__logo {
  width: 26px;
  height: 26px;
}

.menu__title {
  margin: 0;
  font-size: 20px;
  font-weight: 500;
}

.menu__columns {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(160px, 1fr));
  gap: 14px 24px;
}

.menu__group-title {
  margin: 0 0 8px;
  padding-bottom: 6px;
  border-bottom: 1px solid var(--border);
  color: var(--text-dim);
  font-size: 13px;
  font-weight: 500;
}

.menu__item {
  display: flex;
  align-items: center;
  gap: 10px;
  width: 100%;
  padding: 7px 8px;
  border: 0;
  border-radius: 6px;
  background-color: transparent;
  color: var(--text);
  font-size: 14px;
  text-align: left;
  cursor: pointer;
}

.menu__item:hover {
  background-color: var(--hover);
}
</style>
