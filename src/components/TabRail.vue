<script setup lang="ts">
import { Plus, X } from "@lucide/vue";

/**
 * 真正的标签栏：只列**打开着的**标签页。
 *
 * 以前这里列的是仓库里的全部笔记 —— 那是「浏览全部文档」，暂时不做（入口改为
 * `special:newtab`）。现在每个标签页背后是一个地址，点它就是切过去并重新解析。
 */
defineProps<{
  tabs: { address: string; title: string }[];
  active: number;
}>();

const emit = defineEmits<{
  (e: "select", index: number): void;
  (e: "close", index: number): void;
  (e: "new-tab"): void;
}>();
</script>

<template>
  <aside class="rail">
    <button class="rail__new" type="button" title="新建标签页" @click="emit('new-tab')">
      <Plus :size="14" :stroke-width="2" />
    </button>

    <ol class="rail__list">
      <li v-for="(tab, index) in tabs" :key="`${index}-${tab.address}`">
        <div class="rail__item" :class="{ 'rail__item--active': index === active }">
          <button
            class="rail__pick"
            type="button"
            :title="tab.address"
            @click="emit('select', index)"
          >
            {{ tab.title || tab.address }}
          </button>
          <button
            class="rail__close"
            type="button"
            title="关闭"
            @click="emit('close', index)"
          >
            <X :size="12" :stroke-width="2" />
          </button>
        </div>
      </li>
    </ol>
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
  overflow-y: auto;
}

.rail__new {
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

.rail__new:hover {
  background: var(--hover);
  color: var(--text);
}

.rail__list {
  margin: 0;
  padding: 0;
  list-style: none;
}

.rail__item {
  display: flex;
  align-items: center;
  border-radius: 6px;
}

.rail__item:hover {
  background: var(--hover);
}

.rail__item--active {
  background: var(--hover);
}

.rail__pick {
  appearance: none;
  flex: 1 1 auto;
  min-width: 0;
  padding: 6px 8px;
  border: 0;
  background: transparent;
  color: var(--text-dim);
  font-size: 13px;
  line-height: 1.6;
  text-align: left;
  text-overflow: ellipsis;
  overflow: hidden;
  white-space: nowrap;
  cursor: pointer;
}

.rail__item--active .rail__pick {
  color: var(--text);
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
</style>
