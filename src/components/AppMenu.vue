<script setup lang="ts">
/**
 * 顶栏菜单：列出所有特殊页面。
 *
 * 版式参照 MediaWiki 的站点菜单 —— 贴在标题栏下面整幅铺开，顶上一行站点名，
 * 下面按分组竖列条目。**条目来自后端**（`special_pages` 命令）：后端说有哪些页面，
 * 这里就有哪些；显示名与图标从 `special.ts` 取，没登记的会以 `special:<名字>`
 * 出现在「其它」里，不会因为忘了登记而消失。
 */
import { computed } from "vue";
import { FALLBACK_GROUP, metaOf, SPECIAL_GROUPS } from "../special";
import { logoSrc } from "../theme";

const props = defineProps<{
  /** 后端给出的、当前存在的特殊页面名 */
  pages: string[];
  /** 站点名（顶栏那一行） */
  title: string;
}>();

const emit = defineEmits<{
  (e: "open", address: string): void;
  (e: "close"): void;
}>();

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
  <!-- 点面板外面收起；面板本身在标题栏下方整幅铺开 -->
  <div class="menu-backdrop" @click.self="emit('close')">
    <section class="menu">
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
</template>

<style scoped>
.menu-backdrop {
  position: fixed;
  inset: 0;
  z-index: 54;
  /* 压暗后面的页面，让面板成为注意力的中心（与参考一致） */
  background: rgb(0 0 0 / 35%);
}

.menu {
  padding: 18px 26px 26px;
  border-bottom: 1px solid var(--border);
  background: var(--bg);
  box-shadow: 0 18px 44px rgb(0 0 0 / 40%);
  max-height: calc(100vh - 40px);
  overflow-y: auto;
}

.menu__head {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-bottom: 18px;
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
  grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
  gap: 18px 32px;
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
