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
import { KeyRound, Upload } from "@lucide/vue";
import { gpgAvailable } from "../../core/preferences.ts";
import { useKeyRing, when, expiresOf } from "../../composables/useKeyRing.ts";

/**
 * GPG 密钥（`special:keys`）：列出本机密钥环里的密钥，挑选默认项。
 *
 * 这里只摆版面：一枚密钥三段 —— 用户标识与徽章、指纹与时间、三个按钮。
 * 列表从哪来、默认项怎么写回去、导入怎么走文件选择框，都在
 * `composables/useKeyRing.ts` 里；样式在同目录的 `keys.css`。
 *
 * 私钥不会离开密钥环：本页只读公开信息，也不生成密钥。
 */
const {
  keys,
  loading,
  busy,
  problem,
  confirming,
  notice,
  isSigning,
  isEncrypting,
  useForSigning,
  useForEncrypting,
  importKey,
  remove,
} = useKeyRing();
</script>

<template>
  <section class="keys">
    <div class="keys__head">
      <KeyRound :size="20" :stroke-width="1.9"/>
      <h1 class="keys__title">GPG 密钥</h1>
      <span class="keys__count">{{ keys.length }} 把</span>
      <button class="keys__import" type="button" :disabled="busy || !gpgAvailable" @click="importKey">
        <Upload :size="14" :stroke-width="2"/>
        导入密钥
      </button>
    </div>

    <p v-if="!gpgAvailable" class="keys__problem">
      本机未安装 GnuPG（gpg），签名、加密与校验均不可用，此处也无法列出密钥。
      安装 gpg 后重新打开本页即可。
    </p>

    <p class="keys__lead">
      下列密钥来自<strong>本机密钥环</strong>，与当前仓库无关。设为默认后，仅影响此后新建笔记的
      默认策略；已有笔记沿用各自的存储方式，不受影响。
      导入含私钥的文件时，私钥将一并存入本机密钥环，导入结果会明确提示；
      本页<strong>不生成</strong>密钥，也<strong>只删除公钥</strong> —— 如需删除私钥，请使用
      gpg 自行操作。
    </p>

    <p v-if="notice" class="keys__notice">{{ notice }}</p>
    <p v-if="problem" class="keys__problem">{{ problem }}</p>
    <p v-if="loading" class="keys__hint">正在读…</p>
    <p v-else-if="keys.length === 0 && gpgAvailable" class="keys__hint">
      本机密钥环中暂无密钥。可用 <code>gpg --quick-generate-key</code> 生成，或导入现有密钥。
    </p>

    <ol v-else class="keys__list">
      <li v-for="key in keys" :key="key.fingerprint" class="keys__item">
        <div class="keys__line">
          <span class="keys__uids">
            <span v-for="uid in key.uids" :key="uid" class="keys__uid">{{ uid }}</span>
            <span v-if="key.uids.length === 0" class="keys__uid">（没有用户标识）</span>
          </span>

          <span class="keys__badges">
            <span class="keys__badge" :class="{ 'keys__badge--own': key.secret }">
              {{ key.secret ? "有私钥" : "只有公钥" }}
            </span>
            <span v-if="key.expired" class="keys__badge keys__badge--bad">已过期</span>
            <span class="keys__badge">{{ key.trust }}</span>
            <span v-if="isSigning(key)" class="keys__badge keys__badge--on">默认签名</span>
            <span v-if="isEncrypting(key)" class="keys__badge keys__badge--on">默认加密</span>
          </span>
        </div>

        <div class="keys__meta">
          <code class="keys__fingerprint">{{ key.fingerprint }}</code>
          <span class="keys__when">建于 {{ when(key.created) || "不详" }} · {{ expiresOf(key) }}</span>
        </div>

        <div class="keys__actions">
          <button
              type="button"
              class="keys__btn"
              :class="{ 'keys__btn--on': isSigning(key) }"
              :disabled="busy || !key.can_sign || !key.secret"
              :title="key.secret ? '' : '没有私钥就签不了'"
              @click="useForSigning(key)"
          >
            {{ isSigning(key) ? "不再默认签名" : "用作默认签名" }}
          </button>
          <button
              type="button"
              class="keys__btn"
              :class="{ 'keys__btn--on': isEncrypting(key) }"
              :disabled="busy || !key.can_encrypt"
              @click="useForEncrypting(key)"
          >
            {{ isEncrypting(key) ? "不再默认加密" : "用作默认加密" }}
          </button>
          <button
              type="button"
              class="keys__btn keys__btn--danger"
              :disabled="busy || key.secret"
              :title="key.secret ? '含私钥的密钥请用 gpg 删除' : '从本机密钥环中删除这个公钥'"
              @click="remove(key)"
          >
            {{ confirming === key.fingerprint ? "确认删除公钥" : "删除公钥" }}
          </button>
        </div>
      </li>
    </ol>
  </section>
</template>

<style scoped src="./keys.css"/>
