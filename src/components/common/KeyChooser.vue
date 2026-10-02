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
import { computed, onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { keyLabel, shortFingerprint, type GpgKey } from "../../ipc/keys.ts";

/**
 * 挑一把 GPG 密钥。
 *
 * 收的是**指纹**（钥匙串里唯一认得住的那个），显示的是**人认得出的那个**
 * （`姓名 <邮箱> · 指纹首尾`）—— 让作者去手敲一串 40 位的十六进制，
 * 既容易错，也没人记得住自己那把是什么。
 *
 * 存进笔记里的那个值可以是别种写法（邮箱、短 id）：钥匙串认得出来就行。
 * 所以这里会**对一遍**（见 [`sameKey`]），认得出就照旧选上，认不出才多摆一条
 * "本机没有这把钥匙" —— 免得打开一看，明明设过的东西变成了空白。
 */
const props = defineProps<{
  /** 不用密钥时那一条的写法（`不签名` / `不加密` / `不使用`） */
  emptyLabel: string;
  /** 没装 gpg 时整个禁用 */
  disabled?: boolean;
}>();

/** 选中的指纹；空串 = 不用 */
const chosen = defineModel<string | null>({ required: true });

const keys = ref<GpgKey[]>([]);
/** 读不出来（没装 gpg）就只留下"不用"那一条 */
const loading = ref(true);

onMounted(async () => {
  try {
    keys.value = await invoke<GpgKey[]>("gpg_keys");
  } catch (error) {
    console.warn("取 GPG 密钥清单失败：", error);
    keys.value = [];
  } finally {
    loading.value = false;
  }
});

/**
 * 两串标识是不是同一把钥匙。
 *
 * 指纹、短 id、邮箱都能指一把钥匙，所以先按**结尾**比（短 id 是指纹的尾巴），
 * 再按用户标识里的邮箱/姓名比 —— 只为了"打开时选中的是原来那一把"，不做别的判断。
 */
function sameKey(key: GpgKey, wanted: string): boolean {
  const needle = wanted.trim().toLowerCase();
  if (!needle) {
    return false;
  }
  const fingerprint = key.fingerprint.toLowerCase();
  return (
    fingerprint === needle ||
    fingerprint.endsWith(needle) ||
    key.uids.some((uid) => uid.toLowerCase().includes(needle))
  );
}

/** 当前值在清单里认不出（钥匙串里没有这一把）：给它留一条，别让选择框显示成"不用" */
const missing = computed(() => {
  const wanted = chosen.value?.trim() ?? "";
  if (!wanted || keys.value.some((key) => sameKey(key, wanted))) {
    return "";
  }
  return wanted;
});

/** 选择框里的当前值：认得出就用那一把的指纹，认不出就用原样那个值 */
const picked = computed(() => {
  const wanted = chosen.value?.trim() ?? "";
  if (!wanted) {
    return "";
  }
  return keys.value.find((key) => sameKey(key, wanted))?.fingerprint ?? wanted;
});

/** 只有私钥签得了、也只有能加密的钥匙能收信 —— 不合适的照旧列出来，只是标一下 */
function note(key: GpgKey): string {
  const marks: string[] = [];
  if (!key.secret) {
    marks.push("无私钥");
  }
  if (key.expired) {
    marks.push("已过期");
  }
  return marks.length > 0 ? `（${marks.join("，")}）` : "";
}

/** 选了一条：空串就是"不用"，其余按指纹存 */
function onChange(event: Event) {
  const value = (event.target as HTMLSelectElement).value;
  chosen.value = value === "" ? null : value;
}
</script>

<template>
  <select
      class="key-chooser"
      :value="picked"
      :disabled="disabled || loading"
      @change="onChange"
  >
    <option value="">{{ emptyLabel }}</option>
    <option v-for="key in keys" :key="key.fingerprint" :value="key.fingerprint">
      {{ keyLabel(key) }}{{ note(key) }}
    </option>
    <option v-if="missing" :value="missing">
      {{ shortFingerprint(missing) }}（本机没有这把钥匙）
    </option>
  </select>
</template>

<style scoped>
.key-chooser {
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

.key-chooser:focus {
  outline: none;
  border-color: var(--accent-soft);
}

.key-chooser:disabled {
  opacity: 0.5;
}
</style>
