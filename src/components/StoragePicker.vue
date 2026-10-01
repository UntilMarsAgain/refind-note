<script setup lang="ts">
import { computed } from "vue";
import { policyLabel, type Policy } from "../bindings/note.ts";
import { gpgAvailable } from "../core/preferences.ts";
import KeyChooser from "./KeyChooser.vue";

/**
 * "这一份怎么存"——写笔记、传文件都要问的同一件事。
 *
 * 收起来的那个按钮上写着**当前这一套的名字**（`压缩 · GPG 签名`），点开才是各项；
 * 于是不点也能看见"这次会怎么存"，点开是为了改。
 *
 * 口令与策略分开收：策略是**存进仓库**的（从这一版起粘住），口令只活在这次会话里。
 * 所以它们是两个 model —— 谁也不必知道对方怎么保管。
 */
const policy = defineModel<Policy>("policy", { required: true });
/** 口令层要用的口令；只在 `policy.symmetric` 为真时用得上 */
const passphrase = defineModel<string>("passphrase", { default: "" });

const label = computed(() => policyLabel(policy.value));
</script>

<template>
  <details class="picker">
    <summary class="picker__cap" title="这一份怎么存；改了就从这一份起粘住">存储：{{ label }}</summary>

    <div class="picker__body">
      <label class="picker__check">
        <input v-model="policy.compress" type="checkbox"/>
        压缩
      </label>

      <label class="picker__field">
        签名密钥
        <KeyChooser v-model="policy.gpg_sign" empty-label="不签名" :disabled="!gpgAvailable"/>
      </label>

      <label class="picker__field">
        加密密钥
        <KeyChooser v-model="policy.gpg_encrypt" empty-label="不加密" :disabled="!gpgAvailable"/>
      </label>
      <p v-if="!gpgAvailable" class="picker__hint">本机未安装 gpg，签名与加密不可用。</p>

      <label class="picker__check">
        <input v-model="policy.symmetric" type="checkbox"/>
        口令加密
        <span class="picker__hint">口令仅用于本次会话，不写入磁盘</span>
      </label>

      <label v-if="policy.symmetric" class="picker__field">
        口令
        <input v-model="passphrase" type="password" placeholder="本次会话中使用"/>
      </label>
    </div>
  </details>
</template>

<style scoped>
.picker {
  position: relative;
}

.picker__cap {
  list-style: none;
  display: inline-flex;
  gap: 6px;
  align-items: center;
  padding: 5px 10px;
  border: 1px solid var(--border);
  border-radius: 6px;
  color: var(--text-dim);
  font-size: 12.5px;
  white-space: nowrap;
  cursor: pointer;
}

.picker__cap::-webkit-details-marker {
  display: none;
}

.picker[open] .picker__cap {
  border-color: var(--accent-soft);
  color: var(--text);
}

.picker__body {
  position: absolute;
  z-index: 5;
  top: calc(100% + 6px);
  right: 0;
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-width: 300px;
  padding: 12px;
  border: 1px solid var(--border);
  border-radius: 8px;
  background: var(--surface);
  box-shadow: 0 8px 24px rgba(0, 0, 0, 0.28);
  font-size: 12.5px;
}

.picker__check,
.picker__field {
  display: flex;
  gap: 8px;
  align-items: center;
  color: var(--text-dim);
}

.picker__field input[type="password"] {
  flex: 1;
  min-width: 0;
  padding: 5px 8px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: var(--bg);
  color: var(--text);
  font: inherit;
  font-size: 12.5px;
}

.picker__hint {
  color: var(--text-dim);
  font-size: 11.5px;
}
</style>
