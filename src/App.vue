<script setup lang="ts">
import { ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import TitleBar from "./components/TitleBar.vue";
import WindowResizeHandles from "./components/WindowResizeHandles.vue";

const greetMsg = ref("");
const name = ref("");

async function greet() {
  greetMsg.value = await invoke("greet", { name: name.value });
}

// 标题栏上的入口目前是占位，先在代码里留痕，
// 避免以后看代码时误以为「点了没反应」是 bug。
function onSearch() {
  // TODO: 打开搜索面板
}

function onSubmit(value: string) {
  // TODO: 地址栏提交（跳转 / 搜索）
  console.debug("titlebar submit:", value);
}
</script>

<template>
  <WindowResizeHandles />

  <div class="app">
    <TitleBar @search="onSearch" @submit="onSubmit" />

    <main class="app__body">
      <section class="card">
        <h1 class="card__title">标题栏已替换为自绘</h1>
        <p class="card__desc">
          左侧是占位图标、搜索与菜单，中间可直接输入并回车提交，
          右侧为最小化 / 最大化 / 关闭。拖动标题栏可移动窗口，双击可最大化。
        </p>

        <form class="probe" @submit.prevent="greet">
          <input
            v-model="name"
            class="probe__input"
            placeholder="输入内容，验证 Rust 端 IPC 是否正常"
          />
          <button class="probe__btn" type="submit">调用 greet</button>
        </form>
        <p class="probe__out selectable">{{ greetMsg || "—" }}</p>
      </section>
    </main>
  </div>
</template>

<style scoped>
.app {
  display: flex;
  flex-direction: column;
  height: 100vh;
}

.app__body {
  flex: 1 1 auto;
  overflow: auto;
  padding: 24px;
}

.card {
  max-width: 720px;
  padding: 20px 24px;
  border-radius: 12px;
  background: var(--surface);
}

.card__title {
  margin: 0 0 6px;
  font-size: 19px;
  font-weight: 600;
}

.card__desc {
  margin: 0 0 18px;
  color: var(--text-dim);
  font-size: 14px;
}

.probe {
  display: flex;
  gap: 8px;
}

.probe__input {
  flex: 1 1 auto;
  min-width: 0;
  padding: 8px 12px;
  border: 1px solid var(--border);
  border-radius: 8px;
  background: var(--field-bg);
  color: var(--text);
  outline: none;
}

.probe__input:focus {
  border-color: var(--accent-soft);
}

.probe__btn {
  padding: 8px 16px;
  border: 0;
  border-radius: 8px;
  background: var(--accent);
  color: #fff;
  cursor: pointer;
}

.probe__btn:hover {
  background: var(--accent-hover);
}

.probe__out {
  margin: 12px 0 0;
  color: var(--accent-soft);
  font-size: 14px;
}
</style>
