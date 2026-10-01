<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { Upload } from "@lucide/vue";
import { formatBytes, formatTime } from "../bindings/maintenance.ts";
import type { FileEntry, Uploaded } from "../bindings/files.ts";
import { fileReferenceOf, isImage } from "../file-links.ts";
import { saveVaultFile } from "../file-save.ts";
import { flash } from "../notice.ts";
import { clipboardFiles, uploadPasted } from "../paste-files.ts";

/**
 * 附件页（`special:files`）。
 *
 * 上传有三条路：**选文件**（系统对话框）、**粘贴**（这一页上按 Ctrl+V）、
 * **编辑器里插入**（编辑器自己那一行按钮）。字节都可以不走 IPC（选文件时后端自己读），
 * 粘贴的才递过去 —— 而且走的是二进制通道，不做 base64。
 *
 * 笔记里引用的写法就摆在每一行上：复制引用即可。
 */
const files = ref<FileEntry[]>([]);
const loading = ref(false);
const busy = ref(false);
const problem = ref("");

/** 正在改名的那个（标识）与草稿名 */
const renaming = ref("");
const renameText = ref("");
/** 正在"确认删除"的那个（标识）；两步确认 */
const confirming = ref("");

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

/** 收一批文件（粘贴来的） */
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
    flash(`已收进仓库：${names.join("、")}`);
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
    flash(`已收进仓库：${names.join("、")}`);
    await load();
  } catch (reason) {
    problem.value = String(reason);
  } finally {
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
  renaming.value = file.id;
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
    await invoke("rename_file", { id: file.id, name });
    flash(`已改名为 ${name}（笔记里已经写下的旧名字不会自动改）`);
    renaming.value = "";
    await load();
  } catch (reason) {
    problem.value = String(reason);
  } finally {
    busy.value = false;
  }
}

async function remove(file: FileEntry) {
  if (confirming.value !== file.id) {
    confirming.value = file.id;
    return;
  }

  busy.value = true;
  try {
    await invoke("delete_file", { id: file.id });
    flash(`已删除：${file.name}`);
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
    const target = await saveVaultFile(file.id);
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
      附件存在仓库的 <code>db/files/</code> 下。笔记里按<strong>名字</strong>引用：
      <code>![名字](名字)</code>，或者带排版的
      <code>::image src=名字 align=right width=320</code>。
      也可以在这一页上直接按 <strong>Ctrl+V</strong> 粘贴。
    </p>

    <p v-if="problem" class="files__problem">{{ problem }}</p>
    <p v-if="loading" class="files__hint">正在读…</p>
    <p v-else-if="files.length === 0" class="files__hint">
      还没有文件。点「上传文件」选一个，它就会出现在这里。
    </p>

    <ol v-else class="files__list">
      <li v-for="file in files" :key="file.id" class="files__item">
        <span class="files__thumb">
          <img v-if="isImage(file)" :src="`refind://localhost/files/${file.id}`" :alt="file.name"/>
          <span v-else class="files__ext">{{ (file.extension || "?").toUpperCase() }}</span>
        </span>

        <span class="files__body">
          <template v-if="renaming === file.id">
            <input
                v-model="renameText"
                class="files__rename"
                type="text"
                @keydown.enter.prevent="saveRename(file)"
                @keydown.esc="renaming = ''"
            />
          </template>
          <span v-else class="files__name">{{ file.name }}</span>

          <span class="files__meta">
            {{ formatBytes(file.size) }} · {{ file.mime }} · {{ formatTime(file.uploaded) }}
          </span>
          <code class="files__ref">{{ fileReferenceOf(file) }}</code>
        </span>

        <span class="files__actions">
          <template v-if="renaming === file.id">
            <button type="button" class="files__btn files__btn--go" :disabled="busy" @click="saveRename(file)">
              确认改名
            </button>
            <button type="button" class="files__btn" @click="renaming = ''">取消</button>
          </template>
          <template v-else-if="confirming === file.id">
            <button type="button" class="files__btn files__btn--danger" :disabled="busy" @click="remove(file)">
              确认删除
            </button>
            <button type="button" class="files__btn" @click="confirming = ''">取消</button>
          </template>
          <template v-else>
            <button type="button" class="files__btn" @click="copyReference(file)">复制引用</button>
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
  border: 1px solid var(--border);
  border-radius: 6px;
  overflow: hidden;
  background: var(--bg);
}

.files__thumb img {
  max-width: 100%;
  max-height: 100%;
  object-fit: cover;
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
}

.files__name {
  font-size: 13.5px;
  font-weight: 600;
  overflow-wrap: anywhere;
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
