<script setup lang="ts">
import { ref } from "vue";

/**
 * 特殊页面 `special:newtab`。
 *
 * 现在这里是**进入仓库的唯一入口**：刻意不列全部文档（那件事暂时不做），只给一个输入框，
 * 按名字打开。以后要放最近打开、搜索之类的，都回到这一页上加。
 */
const emit = defineEmits<{ (e: "open", value: string): void }>();

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
      输入笔记名打开；名字不存在时进入创建流程。地址栏同样可用。
    </p>
    <div class="newtab__field">
      <input
        v-model="typed"
        class="newtab__input"
        type="text"
        placeholder="笔记名，例如 平陆运河"
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
  color: var(--text);
  font-size: 20px;
  line-height: 1.5;
}

.newtab__hint {
  margin: 0 0 22px;
  color: var(--text-dim);
  font-size: 13px;
  line-height: 1.7;
}

.newtab__field {
  display: flex;
  gap: 8px;
}

.newtab__input {
  flex: 1 1 auto;
  height: 34px;
  padding: 0 12px;
  border: 1px solid var(--border);
  border-radius: 8px;
  background: var(--field-bg);
  color: var(--text);
  font-size: 13.5px;
  outline: none;
}

.newtab__input:focus {
  border-color: var(--accent-soft);
}

.newtab__go {
  height: 34px;
  padding: 0 16px;
  border: 1px solid var(--accent-soft);
  border-radius: 8px;
  background: transparent;
  color: var(--accent-soft);
  font-size: 13px;
  cursor: pointer;
}

.newtab__go:hover {
  background: var(--hover);
}
</style>
