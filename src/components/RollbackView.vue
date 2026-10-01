<script setup lang="ts">
import { ref } from "vue";
import { invoke } from "@tauri-apps/api/core";

/**
 * 回退的二次确认页。
 *
 * 回退**不丢历史** —— 它是把那一版的内容当一个**新提交**写上去，旧记录一条不改。
 * 但它确实会往上写一版，所以不直接执行：与删除一样先落到一页确认上，
 * 而这一页本身也是一个地址（`名称@rollback-版本`）。
 */
const props = defineProps<{
  title: string;
  /** 要回退到哪一版：地址里的版本 token，由后端解释成版本号 */
  reference: string;
}>();

const emit = defineEmits<{
  /** 回退完成：带上后端刚写下的新版本号，上层据此重读 / 跳转 */
  (e: "rolled-back", rev: number): void;
  /** 不退了：回到"查看那一版"（本来就是从那里点过来的） */
  (e: "cancel"): void;
}>();

const busy = ref(false);
const error = ref("");

/** 复制那一版的封装：新提交指向同一个 blob，不需要解锁 */
const copy = ref(false);

async function confirm() {
  busy.value = true;
  error.value = "";

  try {
    const newRev = await invoke<number>("rollback_note", {
      title: props.title,
      reference: props.reference,
      summary: null,
      copy: copy.value,
    });
    emit("rolled-back", newRev);
  } catch (reason) {
    error.value = String(reason);
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <div class="rollback">
    <h1 class="rollback__title">回退「{{ title }}」到第 {{ reference }} 版？</h1>

    <p class="rollback__note">
      第 {{ reference }} 版的内容会成为<strong>新的一版</strong>；这之后写的版本都还在，
      一条都不会被删掉。所以回退本身也可以再被回退。
    </p>

    <label class="rollback__copy">
      <input v-model="copy" type="checkbox" />
      <span>
        复制那一版的封装（不解锁）
        <span class="rollback__copy-hint">
          勾选：把那一版的字节原样复制成新的一版，不需要口令；这一篇往后的保护也跟着
          变成那一版的。不勾选：解锁那一版、读出内容，照这篇当前的保护重写。
        </span>
      </span>
    </label>

    <p v-if="error" class="rollback__error">{{ error }}</p>

    <div class="rollback__actions">
      <button type="button" class="rollback__cancel" :disabled="busy" @click="emit('cancel')">
        不退了
      </button>
      <button type="button" class="rollback__confirm" :disabled="busy" @click="confirm">
        {{ busy ? "正在回退…" : "回退到这一版" }}
      </button>
    </div>
  </div>
</template>

<style scoped>
.rollback__title {
  margin: 28px 0 12px;
  font-size: 22px;
  font-weight: 600;
  overflow-wrap: anywhere;
}

.rollback__note {
  margin: 0;
  color: var(--text-dim);
  font-size: 13.5px;
  line-height: 1.7;
}

.rollback__copy {
  display: flex;
  gap: 8px;
  align-items: flex-start;
  margin: 16px 0 0;
  color: var(--text);
  font-size: 13.5px;
  line-height: 1.6;
  cursor: pointer;
}

.rollback__copy input {
  flex: 0 0 auto;
  /* 复选框与第一行文字对齐，不跟着行高往下沉 */
  margin-top: 4px;
  accent-color: var(--accent);
}

.rollback__copy-hint {
  display: block;
  margin-top: 4px;
  color: var(--text-dim);
  font-size: 12.5px;
  line-height: 1.7;
}

.rollback__error {
  margin: 12px 0 0;
  padding: 12px 14px;
  border: 1px solid var(--danger);
  border-left-width: 3px;
  border-radius: 8px;
  color: var(--text);
  font-size: 13.5px;
}

.rollback__actions {
  display: flex;
  gap: 10px;
  margin: 20px 0 0;
}

.rollback__cancel,
.rollback__confirm {
  padding: 7px 16px;
  border: 1px solid var(--border);
  border-radius: 7px;
  background: var(--surface);
  color: var(--text);
  font: inherit;
  font-size: 13.5px;
  cursor: pointer;
}

.rollback__cancel:hover:not(:disabled) {
  background: var(--hover);
}

/* 回退不是破坏性的（历史都还在），所以用主题色而不是危险色 */
.rollback__confirm {
  border-color: var(--accent-soft);
  color: var(--accent-soft);
}

.rollback__confirm:hover:not(:disabled) {
  background: var(--accent);
  color: var(--text);
}

.rollback__cancel:disabled,
.rollback__confirm:disabled {
  opacity: 0.6;
  cursor: default;
}
</style>
