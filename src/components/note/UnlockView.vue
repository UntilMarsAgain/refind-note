<!--
  Refind Note is a note-taking software.
  Copyright (C) 2026 Until Mars Again

  This program is free software: you can redistribute it and/or modify
  it under the terms of the GNU Affero General Public License as published by
  the Free Software Foundation, either version 3 of the License, or
  (at your option) any later version.

  This program is distributed in the hope that it will be useful,
  but WITHOUT ANY WARRANTY; without even the implied warranty of
  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
  GNU Affero General Public License for more details.

  You should have received a copy of the GNU Affero General Public License
  along with this program.  If not, see <http://www.gnu.org/licenses/>.
-->

<script setup lang="ts">
import { computed, ref } from "vue";
import { Lock, Unlock } from "@lucide/vue";
import { resolveDecrypt } from "../../ipc/lock.ts";

/**
 * 等口令（`名称@unlock`，或者**就地**摆在读不出来的那一面上）。
 *
 * ## 两种摆法，一个组件
 *
 * - 地址栏敲 `名字@unlock` → 它是整页（`inline` 为假）；
 * - 打开一篇加密笔记 → 它就摆在**那一篇的位置上**（`inline` 为真）。
 *     这是作者要的：原来读一篇加密笔记会把人整个送到 `@unlock` 那个地址去，
 *     于是"我在读哪一篇"在过程中丢了。
 *
 * 两处共用一个组件，所以框的样子、逃生路、"再输一次"的说法都不会分家。
 *
 * ## 为什么走 `resolveDecrypt` 而不是 `unlock`
 *
 * `unlock`（`core/preferences.ts`）只**交口令**，交完就回来 —— 它证明不了成不成。
 * 口令层还好（`unlock` 后端当场验，错的会抛），但 **gpg 那一层根本没有口令**，
 * 于是失败只体现在后面某次读取上，表现为一句技术话，人既没有重试也没有按钮。
 *
 * `resolveDecrypt` 交完口令会**真的去加载一次**，失败时给一句人话（`reason`）——
 * 那才是能作数的一步。见 `src-tauri/src/commands/lock.rs` 抬头。
 */
const props = defineProps<{
  title: string;
  /** 要解哪一版；token 字符串，null = 最新版 */
  reference: string | null;
  /** 就地摆在读不出来的正文位置（省掉"返回这一篇"那颗 —— 已经在这一篇上了） */
  inline?: boolean;
  /** 要不要口令输入框。**由后端给**：对称层为真；gpg 层为假 —— 它问的是钥匙串或智能卡 */
  needsPassphrase?: boolean;
  /** 后端给的原因（原文，给人看） */
  reason?: string;
  /** 刚才是"口令不对"——据此说"再输一次" */
  wrongPassphrase?: boolean;
}>();

const emit = defineEmits<{
  /** 解开了：上层去重读（它自己知道该重读哪一份） */
  (e: "unlocked"): void;
  /** 去别的地方（历史 / 删除……）：地址归上层管 */
  (e: "navigate", input: string): void;
  /** 不解了：回到这一篇的阅读页（它会照旧说"需要口令"）。就地模式下没有这颗 */
  (e: "cancel"): void;
}>();

const passphrase = ref("");
const busy = ref(false);
/** 失败时那句话。**留���输入框** —— 没有"再输一次"的机会，比报错本身更难受 */
const problem = ref(props.reason ?? "");
const wantsPassphrase = ref(props.needsPassphrase ?? true);

const heading = computed(() =>
  props.reference === null ? `「${props.title}」需要解锁` : `「${props.title}」第 ${props.reference} 版需要解锁`,
);

/**
 * 提交一次。
 *
 * 出错时**不清输入框**：错口令不该让人重新打一遍。
 */
async function submit() {
  if (busy.value) {
    return;
  }
  busy.value = true;
  problem.value = "";

  try {
    const result = await resolveDecrypt(
      "page",
      props.title,
      props.reference,
      wantsPassphrase.value ? passphrase.value : undefined,
    );
    if (!result.readable) {
      problem.value = result.reason || "还是读不出来";
      // 后端说这一版还要口令（既对称又 gpg，外层解开后里面还要问）：
      // 把输入框补上。少这一步的话人会看到一个"解锁"按钮，按了又被要口令。
      if (result.needs_passphrase && !wantsPassphrase.value) {
        wantsPassphrase.value = true;
      }
      return;
    }
    passphrase.value = "";
    emit("unlocked");
  } catch (reason) {
    problem.value = String(reason);
  } finally {
    busy.value = false;
  }
}
</script>

<template>
  <!--
    `inline` 只改两件事：省掉"返回这一篇"那颗（已经在这一篇上了），
    以及外层不加那一段留白 —— 就地的时候上面就是页头。
  -->
  <section class="unlock" :class="{ 'unlock--inline': inline }">
    <h1 class="unlock__title">
      <Lock :size="20" :stroke-width="1.8" />
      {{ heading }}
    </h1>

    <p v-if="wantsPassphrase" class="unlock__hint">
      此笔记以口令加密存储。口令仅用于本次会话，<strong>不会保存到磁盘</strong>，
      关闭程序后需要重新输入。
    </p>
    <p v-else class="unlock__hint">
      这一篇由 gpg 加密，解锁由系统代理办 —— 它多半会自己就解开（钥匙的口令有缓存，
      或者智能卡碰一下就行）。按一下「解锁」就是让它去试。
    </p>

    <div class="unlock__field">
      <input
        v-if="wantsPassphrase"
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

    <p v-if="problem" class="unlock__error">{{ problem }}</p>

    <!--
      想不起口令、或者这台机器上根本没有 gpg 时，得留几条不用解锁也走得通的路：
      历史版本、回退与删除都只看事件链与明文头，读不出正文照样能做。
    -->
    <p class="unlock__escape-hint">无需解锁亦可进行以下操作：</p>
    <div class="unlock__escapes">
      <button type="button" class="unlock__escape" @click="emit('navigate', title + '@history')">
        版本历史
      </button>
      <button type="button" class="unlock__escape" @click="emit('navigate', title + '@delete')">
        删除这一篇
      </button>
      <button
        v-if="!inline"
        class="unlock__back"
        type="button"
        @click="emit('cancel')"
      >
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
