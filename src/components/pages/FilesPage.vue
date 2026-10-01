<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { Lock, Upload } from "@lucide/vue";
import type { FileEntry, Uploaded } from "../../bindings/files.ts";
import { formatBytes, formatTime } from "../../bindings/maintenance.ts";
import { fileReferenceOf } from "../../view/file-links.ts";
import { saveVaultFile } from "../../view/file-save.ts";
import { flash } from "../../core/notice.ts";
import { clipboardFiles, uploadPasted } from "../../view/paste-files.ts";

/**
 * 文件（`special:files`）。
 *
 * 文件就是 `File:` 命名空间里的页面（`File:桥.png`）：传一次是新的一版，
 * **同一个名字再传就是更新**，旧版留在历史里；删除进回收站。所以这一页只管
 * 上传、更新、改名、另存为与删除 —— 版本与删除后的那些事，走的是与笔记同一条路。
 */
const emit = defineEmits<{
  /** 去别的页面（打开某个文件页面） */
  (e: "navigate", input: string): void;
}>();

const files = ref<FileEntry[]>([]);
const loading = ref(false);
const busy = ref(false);
const problem = ref("");

/** 正在改名的那个（标题）与草稿名 */
const renaming = ref("");
const renameText = ref("");
/** 正在"确认删除"的那个（标题）；两步确认 */
const confirming = ref("");
/** 正在"更新"的那个（标题）：更新要选一个文件，选之前先把名字记下来 */
const updating = ref("");

async function load() {
  loading.value = true;
  problem.value = "";
  try {
    files.value = await invoke<FileEntry[]>("list_files");
  } catch (reason) {
    files.value = [];
    problem.value = String(reason);
  } finally {
    loading.value = false;
  }
}

onMounted(() => {
  void load();
  window.addEventListener("paste", onPaste);
});

onBeforeUnmount(() => window.removeEventListener("paste", onPaste));

/** 这一页上按 Ctrl+V：把剪贴板里的文件收进来 */
function onPaste(event: ClipboardEvent) {
  const picked = clipboardFiles(event);
  if (picked.length === 0) {
    return;
  }
  event.preventDefault();
  void collect(picked);
}

/** 收一批粘贴来的文件 */
async function collect(picked: File[]) {
  busy.value = true;
  problem.value = "";
  try {
    const names: string[] = [];
    await uploadPasted(picked, async (bytes, name) => {
      const uploaded = await invoke<Uploaded>("upload_bytes", bytes, {
        headers: { "x-file-name": encodeURIComponent(name) },
      });
      names.push(uploaded.entry.name);
    });
    flash(`已上传：${names.join("、")}`);
    await load();
  } catch (reason) {
    problem.value = String(reason);
  } finally {
    busy.value = false;
  }
}

/** 选文件上传：路径交给后端，字节不经过前端 */
async function pick() {
  const picked = await open({ multiple: true, title: "选择要上传的文件" });
  if (!picked) {
    return;
  }
  const paths = Array.isArray(picked) ? picked : [picked];

  busy.value = true;
  problem.value = "";
  try {
    const names: string[] = [];
    for (const path of paths) {
      const uploaded = await invoke<Uploaded>("upload_file", { path });
      names.push(uploaded.entry.name);
    }
    flash(`已上传：${names.join("、")}`);
    await load();
  } catch (reason) {
    problem.value = String(reason);
  } finally {
    busy.value = false;
  }
}

/** 给一个已有的文件传新版：这就等于"更新"，旧版留在历史里 */
async function update(file: FileEntry) {
  const path = await open({ multiple: false, title: `选择「${file.name}」的新内容` });
  if (!path || Array.isArray(path)) {
    return;
  }

  updating.value = file.title;
  busy.value = true;
  try {
    const uploaded = await invoke<Uploaded>("update_file", { title: file.title, path });
    flash(`已更新「${file.name}」到第 ${uploaded.entry.rev} 版（旧版仍在历史里）`);
    await load();
  } catch (reason) {
    problem.value = String(reason);
  } finally {
    updating.value = "";
    busy.value = false;
  }
}

async function copyReference(file: FileEntry) {
  const reference = fileReferenceOf(file);
  try {
    await writeText(reference);
    flash(`已复制引用：${reference}`);
  } catch (error) {
    flash(`复制失败：${error}`);
  }
}

function startRename(file: FileEntry) {
  renaming.value = file.title;
  renameText.value = file.name;
}

async function saveRename(file: FileEntry) {
  const name = renameText.value.trim();
  if (!name || name === file.name) {
    renaming.value = "";
    return;
  }

  busy.value = true;
  try {
    await invoke("rename_file", { title: file.title, name });
    flash(`已改名为「${name}」；笔记里已写下的旧名称不会随之更改`);
    renaming.value = "";
    await load();
  } catch (reason) {
    problem.value = String(reason);
  } finally {
    busy.value = false;
  }
}

async function remove(file: FileEntry) {
  if (confirming.value !== file.title) {
    confirming.value = file.title;
    return;
  }

  busy.value = true;
  try {
    await invoke("delete_file", { title: file.title });
    flash(`已删除「${file.name}」，可在回收站还原`);
    confirming.value = "";
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
</script>

<template>
  <section class="files">
    <div class="files__head">
      <h1 class="files__title">文件</h1>
      <span class="files__count">{{ files.length }} 个</span>
      <button class="files__upload" type="button" :disabled="busy" @click="pick">
        <Upload :size="14" :stroke-width="2"/>
        {{ busy ? "上传中…" : "上传文件" }}
      </button>
    </div>

    <p class="files__lead">
      文件保存在仓库里，与笔记同一条路：<strong>同一个名字再传一次就是更新</strong>，
      旧版留在历史里；删除进回收站。笔记里按<strong>名称</strong>引用：
      <code>![名称](名称)</code>，或使用图片排版语法
      <code>::image src=名称 align=right width=320</code>。也可以在此页直接按
      <strong>Ctrl+V</strong> 粘贴上传。
    </p>

    <p v-if="problem" class="files__problem">{{ problem }}</p>
    <p v-if="loading" class="files__hint">正在读…</p>
    <p v-else-if="files.length === 0" class="files__hint">
      暂无文件。点击「上传文件」选择，或直接粘贴。
    </p>

    <ol v-else class="files__list">
      <li v-for="file in files" :key="file.title" class="files__item">
        <button
            class="files__thumb"
            type="button"
            :title="`打开「${file.name}」这一页`"
            @click="emit('navigate', file.title)"
        >
          <img
              v-if="file.mime.startsWith('image/') && !file.needs_unlock"
              :src="file.url"
              :alt="file.name"
          />
          <span v-else-if="file.needs_unlock" class="files__lock" title="加密存的：打开这一页解锁">
            <Lock :size="13" :stroke-width="1.9"/>
          </span>
          <span v-else class="files__ext">{{ (file.name.split(".").pop() ?? "?").toUpperCase() }}</span>
        </button>

        <span class="files__body">
          <template v-if="renaming === file.title">
            <input
                v-model="renameText"
                class="files__rename"
                type="text"
                @keydown.enter.prevent="saveRename(file)"
                @keydown.esc="renaming = ''"
            />
          </template>
          <button
              v-else
              type="button"
              class="files__name"
              @click="emit('navigate', file.title)"
          >
            {{ file.name }}
          </button>

          <span class="files__meta">
            第 {{ file.rev }} 版 · {{ formatBytes(file.size) }} · {{ file.mime }} ·
            改于 {{ formatTime(file.modified) }}
          </span>
          <code class="files__ref">{{ fileReferenceOf(file) }}</code>
        </span>

        <span class="files__actions">
          <template v-if="renaming === file.title">
            <button type="button" class="files__btn files__btn--go" :disabled="busy" @click="saveRename(file)">
              确认改名
            </button>
            <button type="button" class="files__btn" @click="renaming = ''">取消</button>
          </template>
          <template v-else-if="confirming === file.title">
            <button type="button" class="files__btn files__btn--danger" :disabled="busy" @click="remove(file)">
              确认删除
            </button>
            <button type="button" class="files__btn" @click="confirming = ''">取消</button>
          </template>
          <template v-else>
            <button type="button" class="files__btn" @click="copyReference(file)">复制引用</button>
            <button type="button" class="files__btn" :disabled="busy" @click="update(file)">
              {{ updating === file.title ? "更新中…" : "更新" }}
            </button>
            <button type="button" class="files__btn" @click="startRename(file)">重命名</button>
            <button type="button" class="files__btn" @click="saveAs(file)">另存为…</button>
            <button type="button" class="files__btn files__btn--danger" @click="remove(file)">
              删除
            </button>
          </template>
        </span>
      </li>
    </ol>
  </section>
</template>

<style scoped>
.files {
  padding-top: 18px;
}

.files__head {
  display: flex;
  flex-wrap: wrap;
  gap: 12px;
  align-items: baseline;
}

.files__title {
  margin: 0;
  font-size: 1.7em;
}

.files__count {
  color: var(--text-dim);
  font-size: 13px;
}

.files__upload {
  display: inline-flex;
  gap: 6px;
  align-items: center;
  margin-left: auto;
  padding: 6px 12px;
  border: 1px solid var(--accent-soft);
  border-radius: 6px;
  background: transparent;
  color: var(--accent-soft);
  font: inherit;
  font-size: 13px;
  cursor: pointer;
}

.files__upload:hover:not(:disabled) {
  background: var(--accent);
  color: var(--text);
}

.files__upload:disabled {
  opacity: 0.5;
  cursor: default;
}

.files__lead {
  margin: 10px 0 18px;
  color: var(--text-dim);
  font-size: 0.95em;
  line-height: 1.8;
}

.files__lead code {
  padding: 1px 5px;
  border-radius: 4px;
  background: var(--hover);
  font-family: var(--mono-font);
  font-size: 0.9em;
}

.files__problem {
  margin: 0 0 12px;
  padding: 8px 12px;
  border-left: 3px solid var(--danger);
  color: var(--text);
  font-size: 13px;
}

.files__hint {
  color: var(--text-dim);
  font-size: 13.5px;
}

.files__list {
  margin: 0;
  padding: 0;
  list-style: none;
  border: 1px solid var(--border);
  border-radius: 8px;
  overflow: hidden;
}

.files__item {
  display: flex;
  gap: 12px;
  align-items: center;
  padding: 10px 12px;
  border-top: 1px solid var(--border);
}

.files__item:first-child {
  border-top: 0;
}

.files__item:nth-child(odd) {
  background: var(--surface);
}

.files__thumb {
  display: inline-flex;
  flex: 0 0 auto;
  align-items: center;
  justify-content: center;
  width: 44px;
  height: 44px;
  padding: 0;
  border: 1px solid var(--border);
  border-radius: 6px;
  overflow: hidden;
  background: var(--bg);
  cursor: pointer;
}

.files__thumb:hover {
  border-color: var(--accent-soft);
}

.files__thumb img {
  max-width: 100%;
  max-height: 100%;
  object-fit: cover;
}

.files__lock {
  display: inline-flex;
  align-items: center;
  color: var(--accent-soft);
}

.files__ext {
  color: var(--text-dim);
  font-family: var(--mono-font);
  font-size: 11px;
}

.files__body {
  display: flex;
  flex: 1 1 auto;
  flex-direction: column;
  gap: 3px;
  min-width: 0;
  align-items: flex-start;
}

.files__name {
  padding: 0;
  border: 0;
  background: transparent;
  color: var(--text);
  font: inherit;
  font-size: 13.5px;
  font-weight: 600;
  text-align: left;
  overflow-wrap: anywhere;
  cursor: pointer;
}

.files__name:hover {
  color: var(--accent-soft);
  text-decoration: underline;
}

.files__rename {
  height: 26px;
  padding: 0 8px;
  border: 1px solid var(--accent-soft);
  border-radius: 6px;
  background: var(--field-bg);
  color: var(--text);
  font: inherit;
  font-size: 13px;
}

.files__rename:focus {
  outline: none;
}

.files__meta {
  color: var(--text-dim);
  font-size: 12px;
}

.files__ref {
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

.files__actions {
  display: flex;
  flex: 0 0 auto;
  flex-wrap: wrap;
  gap: 6px;
  justify-content: flex-end;
}

.files__btn {
  padding: 4px 10px;
  border: 1px solid var(--border);
  border-radius: 999px;
  background: transparent;
  color: var(--text-dim);
  font: inherit;
  font-size: 12px;
  white-space: nowrap;
  cursor: pointer;
}

.files__btn:hover:not(:disabled) {
  border-color: var(--accent-soft);
  background: var(--accent-tint);
  color: var(--text);
}

.files__btn--go {
  border-color: var(--accent-soft);
  color: var(--accent-soft);
}

.files__btn--danger:hover:not(:disabled) {
  border-color: var(--danger);
  background: transparent;
  color: var(--danger);
}

.files__btn:disabled {
  opacity: 0.5;
  cursor: default;
}
</style>
