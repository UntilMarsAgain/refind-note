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
import {
  policyFrom,
  policyLabel,
  type Policy,
  type RevisionSummary,
} from "../../ipc/note.ts";
import { gpgAvailable, protection as repoProtection } from "../../core/preferences.ts";

/**
 * 回退的二次确认页。
 *
 * 回退**不丢历史** —— 它是把那一版的内容当一个**新提交**写上去，旧记录一条不改。
 * 但它确实会往上写一版，所以不直接执行：与删除一样先落到一页确认上，
 * 而这一页本身也是一个地址（`名称@rollback-版本`）。
 *
 * 两条路：
 * - 重写（默认）：解锁那一版、读出内容、按**这一页选的封装**写新的一版；
 * - 复制：把那一版的字节原样复制过来，不解锁、不改封装。
 */
const props = defineProps<{
  title: string;
  /** 要回退到哪一版：地址里的版本 token，由后端解释成版本号 */
  reference: string;
}>();

const emit = defineEmits<{
  /** 回退完成：带上后端刚写下的新版本号，上层据此重读 / 跳转 */
  (e: "rolled-back", rev: number): void;
  /** 取消：回到"查看那一版"（本来就是从那里点过来的） */
  (e: "cancel"): void;
}>();

const busy = ref(false);
const error = ref("");

/** 复制那一版的封装：新提交指向同一个 blob，不需要解锁 */
const copy = ref(false);

/**
 * 新的一版怎么存。
 *
 * 默认照这篇**当前**（最新一版）的保护 —— 与编辑页同一条规矩：回退不该顺手换掉封装。
 * 想换就在这儿显式改，从新这一版起粘住。
 */
const perCommit = ref<Policy>({ ...repoProtection.value });
/** 这一版要套对称层时的口令；交给后端会话后就不再留着 */
const passphraseDraft = ref("");

const chosenLabel = computed(() => policyLabel(perCommit.value));

/**
 * 默认值取这篇最新一版的保护。
 *
 * 历史清单报得出每一版的封装，**不需要解锁** —— 所以就算当前这一版读不出来，
 * 也照样说得清"回退时默认照什么存"。
 */
async function loadPolicy() {
  try {
    const revisions = await invoke<RevisionSummary[]>("list_revisions", {
      title: props.title,
    });
    // 清单是**新的在前**
    const latest = revisions[0];
    if (latest) {
      perCommit.value = policyFrom(latest.protection);
    }
  } catch (reason) {
    // 取不到就照仓库默认；这不该成为"退不回去"的理由
    console.debug("取这篇的保护失败：", reason);
  }
}

onMounted(() => void loadPolicy());

async function confirm() {
  busy.value = true;
  error.value = "";

  try {
    const newRev = await invoke<number>("rollback_note", {
      title: props.title,
      reference: props.reference,
      summary: null,
      copy: copy.value,
      // 复制那一支整个封装跟着旧版，保护与口令都轮不到它说话
      protection: copy.value ? null : perCommit.value,
      passphrase: !copy.value && perCommit.value.symmetric ? passphraseDraft.value || null : null,
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
      第 {{ reference }} 版的内容将成为<strong>最新一版</strong>，其后的版本记录全部保留，
      因此回退本身也可以再次回退。
    </p>

    <label class="rollback__copy">
      <input v-model="copy" type="checkbox"/>
      <span>
        沿用该版本的存储方式（无需解锁）
        <span class="rollback__copy-hint">
          勾选：直接复制该版本的内容，不改变存储方式，也无需输入口令，此后该笔记沿用这一方式。
          不勾选：需解锁并读取内容，按下方选定的方式重新写入。
        </span>
      </span>
    </label>

    <p v-if="copy" class="rollback__frozen">
      存储方式沿用第 {{ reference }} 版，无需另行选择。
    </p>

    <details v-else class="rconf">
      <summary class="rconf__cap">新的存储方式：{{ chosenLabel }}</summary>

      <div class="rconf__body">
        <label class="rconf__check">
          <input v-model="perCommit.compress" type="checkbox"/>
          压缩
        </label>

        <label class="rconf__field">
          签名密钥
          <input
              v-model="perCommit.gpg_sign"
              type="text"
              placeholder="留空表示不签名"
              :disabled="!gpgAvailable"
          />
        </label>

        <label class="rconf__field">
          加密密钥
          <input
              v-model="perCommit.gpg_encrypt"
              type="text"
              placeholder="留空表示不加密"
              :disabled="!gpgAvailable"
          />
        </label>
        <p v-if="!gpgAvailable" class="rconf__hint">本机未安装 gpg，签名与加密不可用。</p>

        <label class="rconf__check">
          <input v-model="perCommit.symmetric" type="checkbox"/>
          口令加密
          <span class="rconf__hint">口令仅用于本次会话，不写入磁盘</span>
        </label>

        <label v-if="perCommit.symmetric" class="rconf__field">
          口令
          <input v-model="passphraseDraft" type="password" placeholder="本次会话中使用"/>
        </label>
      </div>
    </details>

    <p v-if="error" class="rollback__error">{{ error }}</p>

    <div class="rollback__actions">
      <button type="button" class="rollback__cancel" :disabled="busy" @click="emit('cancel')">
        取消
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

.rollback__frozen {
  margin: 12px 0 0;
  color: var(--text-dim);
  font-size: 13px;
}

/* 「新封装」这一栏：默认照这篇当前的保护，改了就从新这一版起粘住 */
.rconf {
  margin: 16px 0 0;
}

.rconf__cap {
  display: inline-block;
  padding: 5px 10px;
  border: 1px solid var(--border);
  border-radius: 6px;
  color: var(--text-dim);
  font-size: 12.5px;
  cursor: pointer;
}

.rconf[open] .rconf__cap {
  border-color: var(--accent-soft);
  color: var(--text);
}

.rconf__body {
  display: flex;
  flex-direction: column;
  gap: 8px;
  max-width: 420px;
  margin: 10px 0 0;
  padding: 12px;
  border: 1px solid var(--border);
  border-radius: 8px;
  background: var(--surface);
  font-size: 12.5px;
}

.rconf__check,
.rconf__field {
  display: flex;
  gap: 8px;
  align-items: center;
  color: var(--text-dim);
}

.rconf__field input {
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

.rconf__hint {
  color: var(--text-dim);
  font-size: 11.5px;
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
