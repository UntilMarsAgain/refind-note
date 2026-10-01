<script setup lang="ts">
import { onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { KeyRound, Upload } from "@lucide/vue";
import { flash } from "../../core/notice.ts";
import { gpgAvailable, protection, setProtection } from "../../core/preferences.ts";

/**
 * GPG 密钥（`special:keys`）：列出钥匙串里的钥匙，挑一把当默认。
 *
 * 三件事：**看得见**（指纹、用户标识、信任程度、能不能签 / 加密、什么时候过期）、
 * **选得中**（设为仓库默认的签名密钥 / 加密密钥）、**管得了**（从文件导入公钥、
 * 删掉不再需要的公钥）。
 *
 * 私钥不会离开钥匙串：这一页只读公开信息，也不生成密钥 —— 那是 `gpg` 自己的活。
 */
interface GpgKey {
  fingerprint: string;
  uids: string[];
  trust: string;
  secret: boolean;
  can_sign: boolean;
  can_encrypt: boolean;
  created: string;
  expires: string;
  expired: boolean;
}

const keys = ref<GpgKey[]>([]);
const loading = ref(false);
const busy = ref(false);
const problem = ref("");
/** 正在"确认删除"的那一把（指纹）；两步确认 */
const confirming = ref("");
/** 导入之后的提醒（带了私钥这种事必须说出来） */
const notice = ref("");

async function load() {
  loading.value = true;
  problem.value = "";
  try {
    keys.value = await invoke<GpgKey[]>("gpg_keys");
  } catch (reason) {
    keys.value = [];
    problem.value = String(reason);
  } finally {
    loading.value = false;
  }
}

onMounted(() => void load());

/** 这一把是不是当前的默认签名 / 加密密钥 */
function isSigning(key: GpgKey): boolean {
  return protection.value.gpg_sign === key.fingerprint;
}

function isEncrypting(key: GpgKey): boolean {
  return protection.value.gpg_encrypt === key.fingerprint;
}

/** 设为默认：同一样再点一次就是取消（回到"不用"） */
async function useForSigning(key: GpgKey) {
  busy.value = true;
  try {
    await setProtection({
      ...protection.value,
      gpg_sign: isSigning(key) ? null : key.fingerprint,
    });
    flash(
      isSigning(key)
        ? `以后不再默认签名`
        : `以后新笔记默认用 ${short(key.fingerprint)} 签名`,
    );
  } catch (error) {
    flash(`改默认签名密钥失败：${error}`);
  } finally {
    busy.value = false;
  }
}

async function useForEncrypting(key: GpgKey) {
  busy.value = true;
  try {
    await setProtection({
      ...protection.value,
      gpg_encrypt: isEncrypting(key) ? null : key.fingerprint,
    });
    flash(
      isEncrypting(key)
        ? `以后不再默认加密`
        : `以后新笔记默认加密到 ${short(key.fingerprint)}`,
    );
  } catch (error) {
    flash(`改默认加密密钥失败：${error}`);
  } finally {
    busy.value = false;
  }
}

async function importKey() {
  // 私钥文件同样能导（gpg 自己分得清）：文件里带了私钥，导完要明说一声
  const picked = await open({
    multiple: false,
    title: "选择要导入的钥匙文件（公钥或私钥）",
    filters: [{ name: "钥匙文件", extensions: ["asc", "gpg", "pub", "key", "sec", "skr"] }],
  });
  if (!picked || Array.isArray(picked)) {
    return;
  }

  busy.value = true;
  try {
    const summary = await invoke<{
      imported: number;
      unchanged: number;
      secret_imported: number;
      secret_unchanged: number;
    }>("import_gpg_key", { path: picked });

    const parts = [`新收 ${summary.imported} 把`, `没有变化的 ${summary.unchanged} 把`];
    if (summary.secret_imported > 0) {
      parts.push(`其中 ${summary.secret_imported} 把带着私钥`);
    }
    flash(`导入完成：${parts.join("，")}`);

    // 私钥进来了是件大事：从此这台机器能替那个人签名、解密
    notice.value =
      summary.secret_imported > 0
        ? `这次导入带来了 ${summary.secret_imported} 把私钥：它们落在本机钥匙串里，这台机器从此能替它们签名与解密。用不上的话，请用 gpg 把私钥删掉（这一页只删公钥）。`
        : "";
    await load();
  } catch (error) {
    problem.value = String(error);
  } finally {
    busy.value = false;
  }
}

async function remove(key: GpgKey) {
  if (confirming.value !== key.fingerprint) {
    confirming.value = key.fingerprint;
    return;
  }

  busy.value = true;
  try {
    await invoke("delete_gpg_key", { fingerprint: key.fingerprint });
    flash(`已删除公钥 ${short(key.fingerprint)}`);
    confirming.value = "";
    await load();
  } catch (error) {
    problem.value = String(error);
  } finally {
    busy.value = false;
  }
}

/** 指纹太长，界面上只显示首尾；要整条就复制 */
function short(fingerprint: string): string {
  return fingerprint.length > 16
    ? `${fingerprint.slice(0, 8)}…${fingerprint.slice(-8)}`
    : fingerprint;
}

function when(at: string): string {
  if (!at) {
    return "";
  }
  const stamp = new Date(at);
  return Number.isNaN(stamp.getTime()) ? at : stamp.toLocaleDateString();
}

function expiresOf(key: GpgKey): string {
  if (key.expired) {
    return "已过期";
  }
  return key.expires ? `到 ${when(key.expires)}` : "永不过期";
}
</script>

<template>
  <section class="keys">
    <div class="keys__head">
      <KeyRound :size="20" :stroke-width="1.9"/>
      <h1 class="keys__title">GPG 密钥</h1>
      <span class="keys__count">{{ keys.length }} 把</span>
      <button class="keys__import" type="button" :disabled="busy || !gpgAvailable" @click="importKey">
        <Upload :size="14" :stroke-width="2"/>
        导入钥匙
      </button>
    </div>

    <p v-if="!gpgAvailable" class="keys__problem">
      这台计算机上没有 gpg：签名、加密与验签都用不了，这一页也列不出钥匙。
      装上 gpg（并确认它在 PATH 里）之后回来即可。
    </p>

    <p class="keys__lead">
      这份清单来自<strong>本机的钥匙串</strong>（不是这个仓库）。点「用作默认签名 / 加密」
      只是改仓库的默认策略 —— 已经写过的笔记照自己最新一版粘住，改它不影响那些。
      导入的文件里若带着私钥，它会落进本机钥匙串（gpg 自己收，程序不代劳），导完会明说是哪几把；
      这一页<strong>不生成</strong>密钥，也<strong>只删公钥</strong>—— 私钥要删请自己用 gpg，
      免得误删了本机主人自己的钥匙。
    </p>

    <p v-if="notice" class="keys__notice">{{ notice }}</p>
    <p v-if="problem" class="keys__problem">{{ problem }}</p>
    <p v-if="loading" class="keys__hint">正在读…</p>
    <p v-else-if="keys.length === 0 && gpgAvailable" class="keys__hint">
      钥匙串里还没有钥匙。用 <code>gpg --quick-generate-key</code> 生成一把，或者导入一份公钥。
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
              :title="key.secret ? '带私钥的钥匙请自己用 gpg 删' : '把这把公钥从钥匙串里删掉'"
              @click="remove(key)"
          >
            {{ confirming === key.fingerprint ? "确认删除公钥" : "删除公钥" }}
          </button>
        </div>
      </li>
    </ol>
  </section>
</template>

<style scoped>
.keys {
  padding-top: 18px;
}

.keys__head {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
  align-items: baseline;
}

.keys__title {
  margin: 0;
  font-size: 1.7em;
}

.keys__count {
  color: var(--text-dim);
  font-size: 13px;
}

.keys__import {
  display: inline-flex;
  gap: 6px;
  align-items: center;
  margin-left: auto;
  padding: 5px 12px;
  border: 1px solid var(--accent-soft);
  border-radius: 6px;
  background: transparent;
  color: var(--accent-soft);
  font: inherit;
  font-size: 13px;
  cursor: pointer;
}

.keys__import:hover:not(:disabled) {
  background: var(--accent);
  color: var(--text);
}

.keys__import:disabled {
  opacity: 0.5;
  cursor: default;
}

.keys__lead {
  margin: 10px 0 18px;
  color: var(--text-dim);
  font-size: 0.95em;
  line-height: 1.8;
}

.keys__problem {
  margin: 12px 0;
  padding: 8px 12px;
  border-left: 3px solid var(--danger);
  color: var(--text);
  font-size: 13px;
  line-height: 1.7;
}

/* 导入带了私钥这类提醒：是提示，不是错误，所以用主题色 */
.keys__notice {
  margin: 12px 0;
  padding: 8px 12px;
  border-left: 3px solid var(--accent-soft);
  border-radius: 6px;
  background: var(--accent-tint);
  color: var(--text);
  font-size: 13px;
  line-height: 1.7;
}

.keys__hint {
  color: var(--text-dim);
  font-size: 13.5px;
}

.keys__list {
  margin: 0;
  padding: 0;
  list-style: none;
  border: 1px solid var(--border);
  border-radius: 8px;
  overflow: hidden;
}

.keys__item {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 10px 12px;
  border-top: 1px solid var(--border);
}

.keys__item:first-child {
  border-top: 0;
}

.keys__item:nth-child(odd) {
  background: var(--surface);
}

.keys__line {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  align-items: baseline;
  justify-content: space-between;
}

.keys__uids {
  display: flex;
  flex-wrap: wrap;
  gap: 4px 10px;
  min-width: 0;
}

.keys__uid {
  font-size: 13.5px;
  font-weight: 600;
  overflow-wrap: anywhere;
}

.keys__badges {
  display: inline-flex;
  flex-wrap: wrap;
  gap: 4px;
}

.keys__badge {
  padding: 1px 7px;
  border: 1px solid var(--border);
  border-radius: 999px;
  color: var(--text-dim);
  font-size: 11.5px;
  white-space: nowrap;
}

.keys__badge--own,
.keys__badge--on {
  border-color: var(--accent-soft);
  color: var(--accent-soft);
}

.keys__badge--bad {
  border-color: var(--danger);
  color: var(--danger);
}

.keys__meta {
  display: flex;
  flex-wrap: wrap;
  gap: 6px 12px;
  align-items: baseline;
}

.keys__fingerprint {
  padding: 1px 6px;
  border-radius: 4px;
  background: var(--hover);
  color: var(--text-dim);
  font-family: var(--mono-font);
  font-size: 11.5px;
  overflow-wrap: anywhere;
  -webkit-user-select: text;
  user-select: text;
}

.keys__when {
  color: var(--text-dim);
  font-size: 12px;
}

.keys__actions {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}

.keys__btn {
  padding: 4px 10px;
  border: 1px solid var(--border);
  border-radius: 999px;
  background: transparent;
  color: var(--text-dim);
  font: inherit;
  font-size: 12.5px;
  cursor: pointer;
}

.keys__btn:hover:not(:disabled) {
  border-color: var(--accent-soft);
  background: var(--accent-tint);
  color: var(--text);
}

.keys__btn--on {
  border-color: var(--accent-soft);
  color: var(--accent-soft);
}

.keys__btn--danger:hover:not(:disabled) {
  border-color: var(--danger);
  background: transparent;
  color: var(--danger);
}

.keys__btn:disabled {
  opacity: 0.45;
  cursor: default;
}
</style>
