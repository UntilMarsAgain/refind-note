<script setup lang="ts">
import { ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { Lock, Unlock } from "@lucide/vue";
import type { Reading } from "../../bindings/note.ts";
import { unlock } from "../../core/preferences.ts";

/**
 * 等待并输入口令（`名称@unlock`）。
 *
 * 读一篇上了锁的笔记时会先落到这一页 —— 口令**按版本存**，同一篇的不同版本可以用
 * 不同的密码，所以在别的版本上解过锁不等于这一版能读。
 *
 * 口令对不对只有接着读一次才知道，所以这里只管把口令交给后端：
 * 提交成功后 emit `unlocked`，由上层去重读并把结果（含 `wrongPassphrase`）递回来。
 */
const props = defineProps<{
  title: string;
  /** 要解哪一版；token 字符串，null = 最新版 */
  reference: string | null;
}>();

const emit = defineEmits<{
  /** 口令已交给后端：上层去重读 */
  (e: "unlocked"): void;
  /** 去别的地方（历史 / 删除……）：地址归上层管 */
  (e: "navigate", input: string): void;
  /** 不解了：回到这一篇的阅读页（它会照旧说"需要口令"） */
  (e: "cancel"): void;
}>();

const passphrase = ref("");
const busy = ref(false);
const error = ref("");
/** 上一次试的口令不对（后端在读的时候已经把它丢掉了） */
const wrong = ref(false);

async function submit() {
  if (!passphrase.value) {
    return;
  }

  busy.value = true;
  error.value = "";
  wrong.value = false;

  try {
    await unlock(props.title, props.reference, passphrase.value);

    // 口令对不对只有接着读一次才知道：读通了才算真的解开，
    // 读不动就把它当作"这把不对"，当场说清楚，不用把人绕回上一页。
    const reading = await invoke<Reading>("read_note", {
      title: props.title,
      reference: props.reference,
    });
    if (reading.state === "locked") {
      wrong.value = true;
      return;
    }

    passphrase.value = "";
    emit("unlocked");
  } catch (reason) {
    error.value = String(reason);
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <section class="unlock">
    <h1 class="unlock__title">
      <Lock :size="20" :stroke-width="1.8" />
      「{{ title }}」{{ reference === null ? "需要口令" : `第 ${reference} 版需要口令` }}
    </h1>

    <p class="unlock__hint">
      这一篇是加密存的。口令只交给这次会话，<strong>不会写进任何文件</strong>；
      程序一关就没了，下次打开还要重新输入。
    </p>

    <div class="unlock__field">
      <input
        v-model="passphrase"
        class="unlock__input"
        type="password"
        placeholder="这一篇的口令"
        autofocus
        @keydown.enter.prevent="submit"
      />
      <button class="unlock__go" type="button" :disabled="busy" @click="submit">
        <Unlock :size="15" :stroke-width="1.9" />
        {{ busy ? "正在解锁…" : "解锁" }}
      </button>
    </div>

    <p v-if="wrong" class="unlock__error">口令不对，再试一次。</p>
    <p v-if="error" class="unlock__error">{{ error }}</p>

    <!--
      想不起口令、或者这台机器上根本没有 gpg 时，得留几条不用解锁也走得通的路：
      历史版本、回退与删除都只看事件链与明文头，读不出正文照样能做。
    -->
    <p class="unlock__escape-hint">这一版读不出来，但下面这些不用先解开它：</p>
    <div class="unlock__escapes">
      <button type="button" class="unlock__escape" @click="emit('navigate', title + '@history')">
        版本历史
      </button>
      <button type="button" class="unlock__escape" @click="emit('navigate', title + '@delete')">
        删除这一篇
      </button>
      <button class="unlock__back" type="button" @click="emit('cancel')">
        返回「{{ title }}」
      </button>
    </div>
  </section>
</template>

<style scoped>
.unlock {
  max-width: 560px;
  margin: 40px 0 0;
}

.unlock__title {
  display: flex;
  gap: 8px;
  align-items: center;
  margin: 0 0 10px;
  font-size: 20px;
  font-weight: 600;
  overflow-wrap: anywhere;
}

.unlock__hint {
  margin: 0 0 16px;
  color: var(--text-dim);
  font-size: 13.5px;
  line-height: 1.75;
}

.unlock__field {
  display: flex;
  gap: 8px;
}

.unlock__input {
  flex: 1;
  padding: 8px 12px;
  border: 1px solid var(--border);
  border-radius: 7px;
  background: var(--surface);
  color: var(--text);
  font: inherit;
  font-size: 13.5px;
}

.unlock__input:focus {
  outline: none;
  border-color: var(--accent-soft);
}

.unlock__go {
  display: inline-flex;
  gap: 6px;
  align-items: center;
  padding: 8px 16px;
  border: 1px solid var(--accent-soft);
  border-radius: 7px;
  background: transparent;
  color: var(--accent-soft);
  font: inherit;
  font-size: 13.5px;
  cursor: pointer;
}

.unlock__go:hover:not(:disabled) {
  background: var(--accent);
  color: var(--text);
}

.unlock__go:disabled {
  opacity: 0.6;
  cursor: default;
}

.unlock__error {
  margin: 12px 0 0;
  padding: 12px 14px;
  border: 1px solid var(--danger);
  border-left-width: 3px;
  border-radius: 8px;
  color: var(--text);
  font-size: 13.5px;
}

.unlock__escape-hint {
  margin: 22px 0 8px;
  color: var(--text-dim);
  font-size: 12.5px;
}

.unlock__escapes {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  align-items: center;
}

.unlock__escape {
  padding: 6px 12px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: transparent;
  color: var(--text-dim);
  font: inherit;
  font-size: 12.5px;
  cursor: pointer;
}

.unlock__escape:hover {
  border-color: var(--accent-soft);
  background: var(--accent-tint);
  color: var(--text);
}

/* 不解了就走：回到这一篇（读不出来这件事已经写在标题里了） */
.unlock__back {
  padding: 6px 12px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: transparent;
  color: var(--text-dim);
  font: inherit;
  font-size: 12.5px;
  cursor: pointer;
}

.unlock__back:hover {
  border-color: var(--accent-soft);
  background: var(--accent-tint);
  color: var(--text);
}
</style>
