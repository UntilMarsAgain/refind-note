<script setup lang="ts">
import { ref } from "vue";
import { invoke } from "@tauri-apps/api/core";

/**
 * 删除的二次确认页。
 *
 * 删除是**软删**：日志挪进 `trash/`，所以还捞得回来。这句话要写给点按钮的人看 ——
 * 不然他会以为这是"永久删除"，要么不敢点，要么以为点了就没了。
 */
const props = defineProps<{
  title: string;
}>();

const emit = defineEmits<{
  /** 删掉了：上层该换个地方待着 */
  (e: "deleted"): void;
  /** 不删了：回到这篇的阅读页 */
  (e: "cancel"): void;
}>();

const busy = ref(false);
const error = ref("");

async function confirm() {
  busy.value = true;
  error.value = "";

  try {
    await invoke("delete_note", { title: props.title });
    emit("deleted");
  } catch (reason) {
    error.value = String(reason);
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <div class="delete">
    <h1 class="delete__title">删除「{{ title }}」？</h1>

    <p class="delete__note">
      正文与历史都会从列表里消失。日志会挪进回收站，<strong>没有真的抹掉</strong>
      —— 以后还能捞回来。
    </p>

    <p v-if="error" class="delete__error">{{ error }}</p>

    <div class="delete__actions">
      <button type="button" class="delete__cancel" :disabled="busy" @click="emit('cancel')">
        取消
      </button>
      <button type="button" class="delete__confirm" :disabled="busy" @click="confirm">
        {{ busy ? "正在删…" : "删除" }}
      </button>
    </div>
  </div>
</template>

<style scoped>
.delete__title {
  margin: 28px 0 12px;
  font-size: 22px;
  font-weight: 600;
  overflow-wrap: anywhere;
}

.delete__note {
  margin: 0;
  color: var(--text-dim);
  font-size: 13.5px;
  line-height: 1.7;
}

.delete__error {
  margin: 12px 0 0;
  padding: 12px 14px;
  border: 1px solid var(--danger);
  border-left-width: 3px;
  border-radius: 8px;
  color: var(--text);
  font-size: 13.5px;
}

.delete__actions {
  display: flex;
  gap: 10px;
  margin: 20px 0 0;
}

.delete__cancel,
.delete__confirm {
  padding: 7px 16px;
  border: 1px solid var(--border);
  border-radius: 7px;
  background: var(--surface);
  color: var(--text);
  font: inherit;
  font-size: 13.5px;
  cursor: pointer;
}

.delete__cancel:hover:not(:disabled) {
  background: var(--hover);
}

/* 破坏性操作给个能认出来的样子：只有它用危险色 */
.delete__confirm {
  border-color: var(--danger);
  color: var(--danger);
}

.delete__confirm:hover:not(:disabled) {
  background: var(--danger);
  color: var(--bg);
}

.delete__cancel:disabled,
.delete__confirm:disabled {
  opacity: 0.6;
  cursor: default;
}
</style>
