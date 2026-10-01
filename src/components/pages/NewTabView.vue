<script setup lang="ts">
import { ref } from "vue";

/**
 * 特殊页面 `special:newtab`。
 *
 * 按名字打开：名字不存在时走"不存在 + 创建"那条路，所以这里只负责把名字送出去，
 * 不自己判断存不存在 —— 那件事归后端。
 *
 * 命名空间直接写在名字里（`special:settings`），不另做选择器：地址栏能写的东西，
 * 这里就能写，两处走的是同一套解析。
 */
const emit = defineEmits<{
  (e: "open", value: string): void;
}>();

const typed = ref("");

function submit() {
  const value = typed.value.trim();
  if (value) {
    emit("open", value);
  }
}
</script>

<template>
  <section class="newtab">
    <h1 class="newtab__title">新标签页</h1>

    <p class="newtab__hint">
      输入笔记名称即可打开，名称不存在时可直接创建；
      以 <code>special:</code> 开头的标识用于打开系统页面。也可直接使用地址栏。
    </p>

    <div class="newtab__field">
      <input
        v-model="typed"
        class="newtab__input"
        type="text"
        placeholder="笔记名称，或 命名空间:笔记名称"
        @keydown.enter.prevent="submit"
      />
      <button class="newtab__go" type="button" @click="submit">打开</button>
    </div>
  </section>
</template>

<style scoped>
.newtab {
  max-width: 560px;
  margin: 48px auto 0;
  text-align: center;
}

.newtab__title {
  margin: 0 0 10px;
  font-size: 22px;
  font-weight: 600;
}

.newtab__hint {
  margin: 0 0 16px;
  color: var(--text-dim);
  font-size: 13.5px;
  line-height: 1.7;
}

.newtab__field {
  display: flex;
  gap: 8px;
}

.newtab__input {
  flex: 1;
  padding: 8px 12px;
  border: 1px solid var(--border);
  border-radius: 7px;
  background: var(--surface);
  color: var(--text);
  font: inherit;
  font-size: 13.5px;
}

.newtab__input:focus {
  outline: none;
  border-color: var(--accent-soft);
}

.newtab__go {
  padding: 8px 18px;
  border: 1px solid var(--accent-soft);
  border-radius: 7px;
  background: transparent;
  color: var(--accent-soft);
  font: inherit;
  font-size: 13.5px;
  cursor: pointer;
}

.newtab__go:hover {
  background: var(--accent);
  color: var(--text);
}
</style>
