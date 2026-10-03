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
import { CIPHER_NOTES } from "../../ipc/note.ts";
import { useSyncSettings } from "../../core/sync-settings.ts";
import { syncBusy } from "../../core/sync.ts";

/**
 * 设置页的「云端同步」一节（`special:settings#sync-*`）。
 *
 * 为什么单独成组件：这一节自己就占了大半页，而且与别的几节**没有一点共用** ——
 * 不碰偏好、不碰仓库的封装策略，只有一份自己的连接信息与密钥。
 * 它连自己的一整块说明文字都自带（密钥为什么不显示、为什么要导出、锁是怎么排队的），
 * 放在一起时那些话会被别的设置挤得很远。
 *
 * 状态与动作都在 `core/sync-settings.ts`（那边答的是"配些什么"，`core/sync.ts`
 * 答的是"什么时候同步"）；这里只管摆那一大块表单与它的说明。
 *
 * 没有 props 以外的依赖，也不往上抛事件：这一节改的是设置，不是页面。
 */
const props = defineProps<{
  /** 地址里的章节：跳到那一项并高亮 */
  focus?: string;
}>();

const {
  sync,
  secretDraft,
  lastSync,
  syncProblem,
  pastedKey,
  saveSync,
  syncNow,
  setSyncCipher,
  generateSyncKey,
  usePastedKey,
  exportKey,
  copyKey,
} = useSyncSettings();

function isFocused(id: string): boolean {
  return props.focus === id;
}
</script>

<template>
  <h2 class="settings__section">云端同步</h2>

  <p class="settings__hint">
    把<strong>仓库</strong>同步到 S3 兼容的服务上（MinIO、Cloudflare R2、对象存储都可以）：
    内容块、事件日志、命名空间表，以及仓库自己的设置。
    <strong>这台机器自己的东西不传</strong>：界面偏好、浏览历史、写了一半的草稿，
    还有这里的密钥本身。
  </p>

  <div id="sync-enabled" class="row" :class="{ 'row--target': isFocused('sync-enabled') }">
    <span class="row__label">启动时同步</span>
    <code class="row__id">#sync-enabled</code>
    <label class="row__check">
      <input
          type="checkbox"
          :checked="sync.enabled"
          @change="sync.enabled = ($event.target as HTMLInputElement).checked; saveSync()"
      />
      <span>每次打开程序先同步一次，再摆界面</span>
    </label>
  </div>

  <div id="sync-endpoint" class="row" :class="{ 'row--target': isFocused('sync-endpoint') }">
    <span class="row__label">服务地址</span>
    <code class="row__id">#sync-endpoint</code>
    <input
        v-model="sync.endpoint"
        class="row__text"
        type="text"
        placeholder="https://s3.example.com 或 https://桶名.s3.example.com"
        @change="saveSync()"
    />
    <span class="row__hint">
      两种写法都认：服务商给的端点带桶名就照填（如缤纷云），程序不会再补一次；
      不带桶名的（自建 MinIO 之类）程序自己补。
    </span>
  </div>

  <div id="sync-bucket" class="row" :class="{ 'row--target': isFocused('sync-bucket') }">
    <span class="row__label">桶与前缀</span>
    <code class="row__id">#sync-bucket</code>
    <input
        v-model="sync.bucket"
        class="row__text"
        type="text"
        placeholder="桶名"
        @change="saveSync()"
    />
    <input
        v-model="sync.prefix"
        class="row__text"
        type="text"
        placeholder="前缀（可留空，例如 refind-note）"
        @change="saveSync()"
    />
  </div>

  <div id="sync-region" class="row" :class="{ 'row--target': isFocused('sync-region') }">
    <span class="row__label">区域</span>
    <code class="row__id">#sync-region</code>
    <input
        v-model="sync.region"
        class="row__text"
        type="text"
        placeholder="us-east-1（多数兼容服务不校验）"
        @change="saveSync()"
    />
  </div>

  <div id="sync-keys" class="row" :class="{ 'row--target': isFocused('sync-keys') }">
    <span class="row__label">密钥</span>
    <code class="row__id">#sync-keys</code>
    <!--
      两把都按密码框显示：Access Key 也是能在服务商控制台之外**不该露在屏幕上**的东西
      （直播、共享屏幕、截图）。它要照着控制台核对，所以值留着，只是打成点；
      点进去全选，直接敲新的就换掉了。
    -->
    <input
        v-model="sync.access_key"
        class="row__text"
        type="password"
        autocomplete="off"
        placeholder="Access Key"
        @focus="($event.target as HTMLInputElement).select()"
        @change="saveSync()"
    />
    <input
        v-model="secretDraft"
        class="row__text"
        type="password"
        autocomplete="new-password"
        :placeholder="sync.has_secret ? 'Secret Key 已设置（留空表示不改）' : 'Secret Key'"
        @change="saveSync()"
    />
  </div>

  <div id="sync-key" class="row" :class="{ 'row--target': isFocused('sync-key') }">
    <span class="row__label">云端加密</span>
    <code class="row__id">#sync-key</code>
    <span class="row__hint">
      {{ sync.has_key ? "密钥已设置 —— 传上去的每一份都是加密的" : "还没有密钥 —— 传上去的是明文" }}
    </span>
    <button class="row__go" type="button" @click="generateSyncKey">
      {{ sync.has_key ? "换一把新密钥" : "生成密钥" }}
    </button>
    <button v-if="sync.has_key" class="row__go" type="button" @click="exportKey">导出到文件…</button>
    <button
      v-if="sync.has_key"
      class="row__go"
      type="button"
      title="直接写进剪贴板（屏幕上不显示）—— 粘到另一台机器上之后记得清掉剪贴板：同一个桌面里的程序都读得到它"
      @click="copyKey"
    >
      复制到剪贴板
    </button>
  </div>

  <div id="sync-cipher" class="row" :class="{ 'row--target': isFocused('sync-cipher') }">
    <span class="row__label">云端算法</span>
    <code class="row__id">#sync-cipher</code>
    <select
      class="row__text"
      :value="sync.cipher"
      :disabled="!sync.has_key"
      @change="setSyncCipher"
    >
      <option v-for="(note, name) in CIPHER_NOTES" :key="name" :value="name">
        {{ name }}（{{ note }}）
      </option>
    </select>
    <span class="row__hint">
      {{ sync.has_key
        ? "云端那一层用哪一档。只影响往后新传的：已经传上去的仍按各自头里记的那一档解。"
        : "先在上面生成或粘一把密钥，再来选这一档。" }}
    </span>
  </div>

  <div class="row">
    <span class="row__label">用别的密钥</span>
    <code class="row__id">#sync-key-paste</code>
    <input
        v-model="pastedKey"
        class="row__text"
        type="text"
        placeholder="把另一台机器上导出的那串密钥粘在这里"
    />
    <button class="row__go" type="button" :disabled="!pastedKey.trim()" @click="usePastedKey">
      用这一把
    </button>
  </div>

  <p class="settings__hint">
    两把钥匙都按密码框显示（打成点），不摆在屏幕上。
    密钥由本程序生成（32 字节随机），<strong>只存在这台机器上，界面上不显示</strong> ——
    显示出来就不只是"碰到电脑才能偷"了：直播、共享屏幕、随手截个图都可能把它带出去。
    要带到别的机器上，用「导出到文件」（那份文件就是钥匙本身，别放进会被同步的目录），
    或者「复制到剪贴板」直接粘过去 —— 剪贴板是公开的，粘完记得清掉。
    换一把密钥意味着云端那些旧密文解不开了，所以<strong>下一次同步会把本机这份整份重传</strong>；
    换算法不用：每一份封装的头里记着自己那一档，<strong>只有往后新传的</strong>才用新选的。
    这一层防的是存储服务与捡到那个桶的人；笔记自身那几层（GPG / 口令）防的是拿到你这台
    机器的人，两件事各管各的。
  </p>

  <div id="sync-run" class="row" :class="{ 'row--target': isFocused('sync-run') }">
    <span class="row__label">同步</span>
    <code class="row__id">#sync-run</code>
    <button class="row__go" type="button" :disabled="syncBusy" @click="syncNow()">
      {{ syncBusy ? "正在同步…" : "立即同步" }}
    </button>
    <button
      class="row__go row__go--force"
      type="button"
      :disabled="syncBusy"
      title="不等云端那把锁：另一边崩在半路、或者你确定它没在同步时用 —— 它要真在同步，两边会撞上"
      @click="syncNow(true)"
    >
      强制同步
    </button>
    <span v-if="lastSync" class="row__hint">{{ lastSync }}</span>
  </div>

  <p v-if="syncProblem" class="settings__problem">{{ syncProblem }}</p>

  <p class="settings__hint">
    密钥只写在<strong>这台机器</strong>的 <code>settings/sync.json</code>（权限只给本人）。
    两台机器同时改同一份仓库时，靠云端一把自旋锁排队；真撞上了，按修改时间取新的那一版，
    并在同步报告里说一声。
    锁被另一边拿着时，「立即同步」就不动手了 —— 它要是崩了，5 分钟没动静就算过期，到时候
    自己拿得过来；<strong>不想等就点「强制同步」</strong>，直接抢（那一边要真在同步，两边会撞上，
    所以是给"对面已经死了"这种时候用的）。
  </p>
</template>

<style scoped src="./rows.css"/>
