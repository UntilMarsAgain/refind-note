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
import { CIPHER_NOTES, COMPRESSION_NOTES, type Cipher, type Compression, type Policy } from "../../ipc/note.ts";
import KeyChooser from "../common/KeyChooser.vue";
import { gpgAvailable, protection, setProtection } from "../../core/preferences.ts";

/**
 * 设置页的「存储」一节（`#storage-compress` / `#storage-sign` / `#storage-encrypt` / `#storage-symmetric`）。
 *
 * 这一节摆的是**仓库默认的封装策略**：新写上去的内容依次经过压缩 → GPG 签名 →
 * GPG 加密 → 口令加密这几层，顺序是这里说的事，不是界面排出来的顺序。
 *
 * 三件容易记错的事，都写在下面那几段注释里：
 *
 * - 它**只对还没有正文的新笔记生效**：已经写过的照自己最新一版粘住（提交时显式换过的那种），
 *   所以改这里的默认值不会去改老笔记，也不会让它们重传一遍；
 * - 换算法**不必重传**：每一份封装的头里记着自己那一档，读的时候照头解；
 * - 口令那层**不落盘** —— 这一层挡的是"拿到你这台机器的人"，与云端同步那一层各管各的。
 */
const props = defineProps<{
  /** 地址里的章节：跳到那一项并高亮 */
  focus?: string;
}>();

function toggleCompress(event: Event) {
  void setProtection({
    ...protection.value,
    compress: (event.target as HTMLInputElement).checked,
  });
}

/** 换压缩算法：只影响此后写的那些（读的时候照每份自己的头解） */
function setCompression(event: Event) {
  void setProtection({
    ...protection.value,
    compression: (event.target as HTMLSelectElement).value as Compression,
  });
}

/** 换口令层的算法 */
function setCipher(event: Event) {
  void setProtection({
    ...protection.value,
    cipher: (event.target as HTMLSelectElement).value as Cipher,
  });
}

function toggleSymmetric(event: Event) {
  void setProtection({
    ...protection.value,
    symmetric: (event.target as HTMLInputElement).checked,
  });
}

/** 挑了签名密钥（`null` = 不签名）。默认策略跟着仓库走，改完立刻写盘 */
function setSign(value: string | null) {
  void setProtection({ ...protection.value, gpg_sign: value });
}

/** 挑了加密密钥（`null` = 不加密） */
function setEncrypt(value: string | null) {
  void setProtection({ ...protection.value, gpg_encrypt: value });
}

/** 从内到外说清这份策略会怎么存；什么都没做就是"原样" */
function policyLabel(policy: Policy): string {
  const layers: string[] = [];
  if (policy.compress) {
    layers.push("压缩");
  }
  if (policy.gpg_sign) {
    layers.push(`GPG 签名 ${policy.gpg_sign}`);
  }
  if (policy.gpg_encrypt) {
    layers.push(`GPG 加密 ${policy.gpg_encrypt}`);
  }
  if (policy.symmetric) {
    layers.push("口令加密");
  }
  return layers.length > 0 ? layers.join(" · ") : "原样";
}

function isFocused(id: string): boolean {
  return props.focus === id;
}
</script>

<template>
  <h2 class="settings__section">存储</h2>

  <p class="settings__note">
    这里是<strong>仓库默认</strong>的封装策略：<strong>只对还没有正文的新笔记生效</strong>。
    已经写过的笔记照自己最新一版粘住 —— 之后每次提交都沿用那一版的方式，直到提交时显式换一次。
    读的时候照每份自己的头解，所以同一个仓库里新旧封装可以并存。
    当前默认：<strong>{{ policyLabel(protection) }}</strong>
  </p>

  <div
    id="storage-compress"
    class="row"
    :class="{ 'row--target': isFocused('storage-compress') }"
  >
    <span class="row__label">压缩</span>
    <code class="row__id">#storage-compress</code>
    <label class="row__check">
      <input type="checkbox" :checked="protection.compress" @change="toggleCompress" />
      <span>存储前先压缩（在签名与加密之前进行）</span>
    </label>
    <!-- 算法只在开着的时候才有意义，也就只在开的时候露出来 -->
    <template v-if="protection.compress">
      <label class="row__check">
        <span>算法</span>
        <select class="row__text" :value="protection.compression" @change="setCompression">
          <option v-for="(note, name) in COMPRESSION_NOTES" :key="name" :value="name">
            {{ name }}（{{ note }}）
          </option>
        </select>
      </label>
    </template>
  </div>

  <div
    id="storage-sign"
    class="row"
    :class="{ 'row--target': isFocused('storage-sign') }"
  >
    <span class="row__label">GPG 签名</span>
    <code class="row__id">#storage-sign</code>
    <KeyChooser
      class="row__text"
      :model-value="protection.gpg_sign"
      empty-label="不签名"
      :disabled="!gpgAvailable"
      @update:model-value="setSign"
    />
    <span v-if="!gpgAvailable" class="row__hint">本机未安装 gpg，签名不可用</span>
  </div>

  <div
    id="storage-encrypt"
    class="row"
    :class="{ 'row--target': isFocused('storage-encrypt') }"
  >
    <span class="row__label">GPG 加密</span>
    <code class="row__id">#storage-encrypt</code>
    <KeyChooser
      class="row__text"
      :model-value="protection.gpg_encrypt"
      empty-label="不加密"
      :disabled="!gpgAvailable"
      @update:model-value="setEncrypt"
    />
    <span v-if="!gpgAvailable" class="row__hint">本机未安装 gpg，加密不可用</span>
  </div>

  <div
    id="storage-symmetric"
    class="row"
    :class="{ 'row--target': isFocused('storage-symmetric') }"
  >
    <span class="row__label">口令加密</span>
    <code class="row__id">#storage-symmetric</code>
    <label class="row__check">
      <input type="checkbox" :checked="protection.symmetric" @change="toggleSymmetric" />
      <span>在最外层附加口令保护（口令由你输入，<strong>不写入磁盘</strong>）</span>
    </label>
    <label v-if="protection.symmetric" class="row__check">
      <span>算法</span>
      <select class="row__text" :value="protection.cipher" @change="setCipher">
        <option v-for="(note, name) in CIPHER_NOTES" :key="name" :value="name">
          {{ name }}（{{ note }}）
        </option>
      </select>
    </label>
    <span v-if="protection.symmetric" class="row__hint">
      新建笔记提交时会要求设置口令，此后阅读这些笔记也需要输入。口令不写入磁盘，遗失后无法恢复。
    </span>
  </div>
</template>

<style scoped src="./rows.css"/>
