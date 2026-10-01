<script setup lang="ts">
import { computed, nextTick, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { Download, ExternalLink, History, Pencil, Trash2, Upload } from "@lucide/vue";
import type { FileEntry, FileInfo, Uploaded } from "../../ipc/files.ts";
import type { Policy } from "../../ipc/note.ts";
import { protection } from "../../core/preferences.ts";
import StoragePicker from "../common/StoragePicker.vue";
import { formatBytes, formatTime } from "../../ipc/maintenance.ts";
import { fileReferenceOf } from "../../dom/file-links.ts";
import { freshUrl } from "../../dom/file-unlock.ts";
import { saveVaultFile } from "../../dom/file-save.ts";
import { flash } from "../../core/notice.ts";
import PageHeader, { type PageAction } from "../note/PageHeader.vue";

/**
 * 文件页面（`File:桥.png`）。
 *
 * 它的正文是**字节**，不是给人读的文本 —— 所以这一页不渲染正文，而是把文件本身
 * 摆出来（能预览的预览，不能的给一句"下载看看"）。能做的事与笔记同源：
 * 看历史、删除（进回收站）、另存为；"改内容"这件事在这里叫**传新版**。
 */
const props = defineProps<{
  /** 显示标题（`File:桥.png`） */
  title: string;
  /** 看哪一版（地址里的版本 token）；null 就是最新一版 */
  reference?: string | null;
  /** 正文滚下去了：页头收起 */
  collapsed: boolean;
  /** 这一页星标过没有 */
  starred?: boolean;
}>();

const emit = defineEmits<{
  (e: "navigate", input: string): void;
  (e: "toggle-star"): void;
}>();

const info = ref<FileInfo | null>(null);
/** 原位输入的口令：只有口令层才用得上 */
const passphrase = ref("");

/** 正在改名：页内输入框摆出来没有 */
const renaming = ref(false);
const renameText = ref("");
const renameEl = ref<HTMLInputElement | null>(null);

/** "用系统应用打开"的明文提示已经摆出来了吗（两步确认，不用系统对话框） */
const confirmingOpen = ref(false);

/** 传新版时这一版怎么存（与上传页、编辑器同一套选择器） */
const uploadPolicy = ref<Policy>({ ...protection.value });
const uploadPassphrase = ref("");

/** 看的是不是一个旧版本 */
const older = computed(() => {
  const found = info.value;
  return props.reference != null && found != null && String(found.rev) !== props.reference;
});
const problem = ref("");
const loading = ref(false);
const busy = ref(false);

/**
 * 读这一份文件的现状。
 *
 * 用的是 `file_info` 而不是 `list_files` —— 它顺带回答"这一版怎么存的、现在读不读得动"，
 * 于是加密的文件不会被当成普通图片直接去拉。
 */
async function load() {
  loading.value = true;
  problem.value = "";
  try {
    info.value = await invoke<FileInfo>("file_info", {
      key: props.title,
      reference: props.reference ?? null,
    });
  } catch (reason) {
    info.value = null;
    problem.value = String(reason);
  } finally {
    loading.value = false;
  }
}

/** 这一份在仓库里是**加密存的**吗（口令层或 gpg 加密层） */
function sealed(file: FileInfo): boolean {
  return file.protection.symmetric || file.protection.encrypt !== null;
}

/** 这一份现在读得动吗 */
const readable = computed(() => {
  const found = info.value;
  if (!found) {
    return false;
  }
  if (!found.needs_unlock) {
    return true;
  }
  // gpg 那一层：直接去读，系统代理自己会问口令
  if (found.needs_secret_key && !found.needs_passphrase) {
    return true;
  }
  return found.passphrase_ready;
});

/** 解锁：把口令交给本次会话，然后重读一遍（这次就显示得出来了） */
async function unlock() {
  const found = info.value;
  if (!found) {
    return;
  }
  busy.value = true;
  try {
    await invoke("unlock", {
      title: found.title,
      // 看的是哪一版就解哪一版（口令按版本存）
      reference: props.reference ?? null,
      passphrase: passphrase.value,
    });
    passphrase.value = "";
    await load();
  } catch (reason) {
    problem.value = String(reason);
  } finally {
    busy.value = false;
  }
}

const entry = computed(() => info.value);

watch(() => [props.title, props.reference], () => void load(), { immediate: true });

const isImage = computed(() => entry.value?.mime.startsWith("image/") ?? false);
const isVideo = computed(() => entry.value?.mime.startsWith("video/") ?? false);
const isAudio = computed(() => entry.value?.mime.startsWith("audio/") ?? false);
const isText = computed(() => entry.value?.mime.startsWith("text/") ?? false);

const actions: PageAction[] = [
  { name: "update", label: "传新版", icon: Upload },
  { name: "save", label: "另存为", icon: Download },
  { name: "history", label: "版本历史", icon: History },
  { name: "rename", label: "改名", icon: Pencil },
  { name: "delete", label: "删除", icon: Trash2, danger: true },
];

function onAction(name: string) {
  const file = entry.value;
  if (!file) {
    return;
  }
  switch (name) {
    case "update":
      void update(file);
      break;
    case "save":
      void saveAs(file);
      break;
    case "history":
      emit("navigate", `${file.title}@history`);
      break;
    case "rename":
      startRename(file);
      break;
    case "delete":
      emit("navigate", `${file.title}@delete`);
      break;
  }
}

/** 传新版：选一个文件，内容替换进来，旧版留在历史里 */
async function update(file: FileEntry) {
  const path = await open({ multiple: false, title: `选择「${file.name}」的新内容` });
  if (!path || Array.isArray(path)) {
    return;
  }

  busy.value = true;
  try {
    const uploaded = await invoke<Uploaded>("update_file", {
      title: file.title,
      path,
      protection: uploadPolicy.value,
      passphrase: uploadPolicy.value.symmetric ? uploadPassphrase.value || null : null,
    });
    flash(`已更新到第 ${uploaded.entry.rev} 版（旧版仍在历史里）`);
    await load();
  } catch (reason) {
    problem.value = String(reason);
  } finally {
    busy.value = false;
  }
}

async function saveAs(file: FileEntry) {
  try {
    const target = await saveVaultFile(file.title, file.name);
    if (target) {
      flash(`已另存为：${target}`);
    }
  } catch (error) {
    flash(`另存失败：${error}`);
  }
}

/**
 * 改名：文件名就是页面名，所以要问一句新名字。
 *
 * 就在这一页上问（一个输入框 + 确认/取消），**不借系统的文件对话框** ——
 * 系统给应用的那种窗口只有"打开/保存文件"，拿它当改名的输入框，
 * 看上去就像要把文件存到哪儿去，答非所问。
 */
function startRename(file: FileEntry) {
  renaming.value = true;
  renameText.value = file.name;
  void nextTick(() => renameEl.value?.select());
}

async function saveRename(file: FileEntry) {
  const name = renameText.value.trim();
  if (!name || name === file.name) {
    renaming.value = false;
    return;
  }

  busy.value = true;
  try {
    const display = await invoke<string>("rename_file", { title: file.title, name });
    flash(`已改名为「${name}」；笔记里已写下的旧名称不会随之更改`);
    renaming.value = false;
    emit("navigate", display);
  } catch (reason) {
    problem.value = String(reason);
  } finally {
    busy.value = false;
  }
}

/**
 * 交给系统的默认应用打开。
 *
 * 系统要的是**路径**，而仓库里存的是字节 —— 所以后端先把这一版落到临时目录里，
 * 再把那个路径交出去（拷贝用完由系统回收，不进仓库）。
 * 加密存的那几种，这一步等于在临时目录里留下一份**明文**：先把这件事说清楚再开。
 */
async function openWithSystem(file: FileInfo) {
  // 加密存的要先问一句：这一步会在临时目录里留下一份**明文**
  if (sealed(file) && !confirmingOpen.value) {
    confirmingOpen.value = true;
    return;
  }
  confirmingOpen.value = false;

  busy.value = true;
  try {
    const staged = await invoke<string>("open_file", {
      title: file.title,
      reference: props.reference ?? null,
    });
    flash(`已交给系统打开（临时副本：${staged}）`);
  } catch (reason) {
    problem.value = String(reason);
  } finally {
    busy.value = false;
  }
}

async function copyReference() {
  const file = entry.value;
  if (!file) {
    return;
  }
  const reference = fileReferenceOf(file);
  try {
    await writeText(reference);
    flash(`已复制引用：${reference}`);
  } catch (error) {
    flash(`复制失败：${error}`);
  }
}
</script>

<template>
  <div class="file">
    <p v-if="loading" class="file__hint">正在读…</p>

    <div v-else-if="problem" class="file__error">
      <p class="file__error-text">{{ problem }}</p>
      <button type="button" class="file__btn" @click="load">重试</button>
    </div>

    <template v-else-if="entry">
      <PageHeader
          :title="entry.title"
          parent=""
          :collapsed="props.collapsed"
          :actions="actions"
          :starred="props.starred ?? false"
          @action="onAction"
          @toggle-star="emit('toggle-star')"
      />

      <p class="file__meta">
        第 {{ entry.rev }} 版 · {{ formatBytes(entry.size) }} · {{ entry.mime }} ·
        改于 {{ formatTime(entry.modified) }}
      </p>

      <!-- 改名：就在这一页上问（系统那种"打开/保存文件"的窗口拿来做改名答非所问） -->
      <div v-if="renaming" class="file__rename">
        <input
            ref="renameEl"
            v-model="renameText"
            class="file__rename-input"
            type="text"
            @keydown.enter.prevent="saveRename(entry)"
            @keydown.esc="renaming = false"
        />
        <button type="button" class="file__btn file__btn--go" :disabled="busy" @click="saveRename(entry)">
          确认改名
        </button>
        <button type="button" class="file__btn" @click="renaming = false">取消</button>
        <span class="file__rename-hint">笔记里已写下的旧名称不会随之更改。</span>
      </div>

      <!-- 看的是历史里的一版：说清楚，并给一条回最新版的路 -->
      <p v-if="older" class="file__older">
        正在看第 {{ entry.rev }} 版，不是最新版。
        <button type="button" class="file__link" @click="emit('navigate', entry.title)">
          回到最新版
        </button>
      </p>

      <!--
        加密的先解锁：**原位输口令**，不跳页 —— 解锁之后这一页自己就刷新了。
        读得动之后：能预览的就地预览，不能预览的给一句实话 + 一个"另存为"。
      -->
      <div v-if="!readable" class="file__locked">
        <p class="file__locked-hint">
          {{ entry.needs_passphrase ? "这一份是加密存的，输入口令后显示。" : "这一份是加密存的，解锁后显示。" }}
        </p>
        <div class="file__locked-row">
          <input
              v-if="entry.needs_passphrase"
              v-model="passphrase"
              class="file__locked-input"
              type="password"
              placeholder="口令"
              @keydown.enter.prevent="unlock"
          />
          <button type="button" class="file__btn file__btn--go" :disabled="busy" @click="unlock">
            解锁并显示
          </button>
        </div>
      </div>

      <template v-else>
        <figure v-if="isImage" class="file__preview">
          <!-- 地址里已经带着版本号（看历史时取的是那一版的字节）；
               `freshUrl` 再加一个时间戳绕开 webview 的缓存 -->
          <img :src="freshUrl(entry.url)" :alt="entry.name"/>
        </figure>
        <figure v-else-if="isVideo" class="file__preview">
          <video :src="entry.url" controls preload="metadata"/>
        </figure>
        <figure v-else-if="isAudio" class="file__preview">
          <audio :src="entry.url" controls preload="metadata"/>
        </figure>
        <p v-else-if="isText" class="file__note">这是文本文件，另存为之后可以打开查看。</p>
        <p v-else class="file__note">
          这个类型不能在这里预览。可以交给系统的默认应用打开，或另存为。
        </p>
      </template>

      <div class="file__actions">
        <button type="button" class="file__btn" :disabled="busy" @click="update(entry)">
          <Upload :size="14" :stroke-width="1.9"/>
          传新版
        </button>
        <template v-if="confirmingOpen">
          <span class="file__warn">
            「{{ entry.name }}」在仓库里是<strong>加密存的</strong>：打开会先解开，
            把一份<strong>明文</strong>写到临时目录再交给外部程序，那一份不受本程序保护。
          </span>
          <button type="button" class="file__btn file__btn--go" :disabled="busy" @click="openWithSystem(entry)">
            确认打开
          </button>
          <button type="button" class="file__btn" @click="confirmingOpen = false">取消</button>
        </template>
        <button v-else type="button" class="file__btn" :disabled="busy" @click="openWithSystem(entry)">
          <ExternalLink :size="14" :stroke-width="1.9"/>
          用系统应用打开
        </button>
        <button type="button" class="file__btn" @click="saveAs(entry)">
          <Download :size="14" :stroke-width="1.9"/>
          另存为…
        </button>
        <button type="button" class="file__btn" @click="copyReference">
          复制引用
        </button>

        <!-- 传新版也要能指定怎么存：与上传页、编辑器同一个选择器 -->
        <StoragePicker v-model:policy="uploadPolicy" v-model:passphrase="uploadPassphrase"/>
      </div>

      <code class="file__ref">{{ fileReferenceOf(entry) }}</code>
    </template>
  </div>
</template>

<style scoped>
.file {
  padding-top: 4px;
}

/* 改名：输入框与两个按钮并排，提示跟在后面 */
.file__rename {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  align-items: center;
  margin: 12px 0 0;
  padding: 10px 12px;
  border: 1px solid var(--border);
  border-radius: 8px;
  background: var(--surface);
}

.file__rename-input {
  flex: 1 1 240px;
  min-width: 0;
  padding: 5px 10px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: var(--field-bg);
  color: var(--text);
  font: inherit;
  font-size: 13px;
}

.file__rename-input:focus {
  outline: none;
  border-color: var(--accent-soft);
}

.file__rename-hint {
  color: var(--text-dim);
  font-size: 12px;
}

/* "会写出明文副本"那一句：说清楚再放行 */
.file__warn {
  flex: 1 1 260px;
  min-width: 0;
  color: var(--text-dim);
  font-size: 12.5px;
  line-height: 1.7;
}

.file__warn strong {
  color: var(--text);
}

.file__older {
  margin: 8px 0 0;
  color: var(--text-dim);
  font-size: 13px;
}

.file__link {
  padding: 0;
  border: 0;
  background: transparent;
  color: var(--accent-soft);
  font: inherit;
  font-size: 13px;
  cursor: pointer;
}

.file__link:hover {
  text-decoration: underline;
}

.file__meta {
  margin: 10px 0 0;
  color: var(--text-dim);
  font-size: 13px;
}

/* 加密文件：先解锁，再谈显示 */
.file__locked {
  margin: 16px 0 0;
  padding: 12px 14px;
  border: 1px solid var(--border);
  border-radius: 8px;
  background: var(--surface);
}

.file__locked-hint {
  margin: 0 0 10px;
  color: var(--text-dim);
  font-size: 13px;
}

.file__locked-row {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.file__locked-input {
  width: 14em;
  padding: 5px 10px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: var(--field-bg);
  color: var(--text);
  font: inherit;
  font-size: 13px;
}

.file__locked-input:focus {
  outline: none;
  border-color: var(--accent-soft);
}

.file__btn--go {
  border-color: var(--accent-soft);
  color: var(--accent-soft);
}

.file__btn--go:hover:not(:disabled) {
  background: var(--accent);
  color: var(--bg);
}

.file__preview {
  margin: 16px 0 0;
}

.file__preview img,
.file__preview video {
  max-width: 100%;
  max-height: 70vh;
  border: 1px solid var(--border);
  border-radius: 8px;
}

.file__preview audio {
  width: 100%;
}

.file__note {
  margin: 16px 0 0;
  color: var(--text-dim);
  font-size: 13.5px;
}

.file__actions {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin: 18px 0 0;
}

.file__btn {
  display: inline-flex;
  gap: 5px;
  align-items: center;
  padding: 5px 12px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: transparent;
  color: var(--text-dim);
  font: inherit;
  font-size: 12.5px;
  cursor: pointer;
}

.file__btn:hover:not(:disabled) {
  border-color: var(--accent-soft);
  background: var(--accent-tint);
  color: var(--text);
}

.file__btn:disabled {
  opacity: 0.5;
  cursor: default;
}

.file__ref {
  display: inline-block;
  margin: 12px 0 0;
  padding: 2px 8px;
  border-radius: 4px;
  background: var(--hover);
  color: var(--text-dim);
  font-family: var(--mono-font);
  font-size: 12px;
}

.file__hint {
  margin: 28px 0 0;
  color: var(--text-dim);
  font-size: 13.5px;
}

.file__error {
  margin: 28px 0 0;
  padding: 12px 14px;
  border: 1px solid var(--danger);
  border-left-width: 3px;
  border-radius: 8px;
  color: var(--text);
  font-size: 13.5px;
}

.file__error-text {
  margin: 0 0 10px;
}
</style>
