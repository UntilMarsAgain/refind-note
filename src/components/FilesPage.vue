<script setup lang="ts">
/**
 * `special:files` —— 附件的浏览、管理与上传。
 *
 * 上传走系统文件选择器给的本机路径：Rust 端直接读文件，不让几 MB 的图片转成 JSON
 * 数组再走一遍 IPC。磁盘上的名字与显示名是分开的（见后端 `storage/files.rs`），
 * 所以这里看到的文件名，就是笔记里引用时该写的名字。
 */
import { nextTick, onBeforeUnmount, onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { FolderOpen, Trash2, Upload } from "@lucide/vue";
import type { FileEntry } from "../bindings";
import { openMenu } from "../context-menu";
import { fileReferenceOf } from "../file-links";
import { clipboardFiles } from "../paste-files";
import { uploadPasted } from "../paste-upload";

const files = ref<FileEntry[]>([]);
const busy = ref(false);
const notice = ref("");
const problem = ref("");
/** 正在等第二次确认的那个附件：删除不可撤销，值得多问一句 */
const confirming = ref("");
/** 正在改名的那个附件（空串表示没有），以及输入框里的草稿 */
const renaming = ref("");
const renameText = ref("");

function formatSize(bytes: number): string {
  if (bytes < 1024) {
    return bytes + " B";
  }
  if (bytes < 1024 * 1024) {
    return (bytes / 1024).toFixed(1) + " KiB";
  }
  return (bytes / (1024 * 1024)).toFixed(1) + " MiB";
}

function isImage(file: FileEntry): boolean {
  return file.mime.startsWith("image/");
}

/** 引用写法在 `file-links.ts` 里定义（编辑器那边用的是同一个） */
const referenceOf = fileReferenceOf;

/**
 * 这一页开着时，Ctrl+V 直接把剪贴板里的图片收进仓库。
 *
 * 挂在窗口上而不是某个输入框：用户刚截完图，注意力在这一页上，不该先去找地方点一下。
 */
function onPaste(event: ClipboardEvent) {
  const files = clipboardFiles(event);
  if (files.length === 0) {
    return;
  }
  event.preventDefault();
  void pasteUpload(files);
}

async function pasteUpload(files: File[]) {
  notice.value = "";
  problem.value = "";
  try {
    const entries = await uploadPasted(files);
    notice.value = "已收进仓库：" + entries.map((entry) => entry.name).join("、");
    await refresh();
  } catch (error) {
    problem.value = String(error);
  }
}

async function refresh() {
  problem.value = "";
  try {
    files.value = await invoke<FileEntry[]>("list_files");
  } catch (error) {
    problem.value = String(error);
  }
}

async function upload() {
  busy.value = true;
  notice.value = "";
  problem.value = "";
  try {
    const picked = await open({ multiple: true, title: "选择要上传的文件" });
    const paths = Array.isArray(picked) ? picked : picked ? [picked] : [];
    if (paths.length === 0) {
      return;
    }
    const names: string[] = [];
    for (const path of paths) {
      const entry = await invoke<FileEntry>("upload_file", { path });
      names.push(entry.name);
    }
    notice.value = "已收进仓库：" + names.join("、");
    await refresh();
  } catch (error) {
    problem.value = String(error);
  } finally {
    busy.value = false;
  }
}

/**
 * 开始改名：输入框拿焦点并**全选** —— 直接打就是新名字，不必先删掉旧的。
 *
 * 名字只是表里的一行（磁盘上是生成的标识），所以改名不动任何文件。
 */
async function beginRename(file: FileEntry) {
  confirming.value = "";
  renaming.value = file.id;
  renameText.value = file.name;
  await nextTick();
  document.querySelector<HTMLInputElement>(".files__rename")?.select();
}

function cancelRename() {
  renaming.value = "";
  renameText.value = "";
}

async function submitRename(file: FileEntry) {
  const wanted = renameText.value.trim();
  // 没改、或者改空了：当作没这回事（空名字后端也会拒绝，不必来回一趟）
  if (wanted === "" || wanted === file.name) {
    cancelRename();
    return;
  }
  problem.value = "";
  try {
    const entry = await invoke<FileEntry>("rename_file", { id: file.id, name: wanted });
    // 旧名字已经写在别人笔记里的话，这里改了并不会跟着变 —— 这句必须说
    notice.value = "已改名为 " + entry.name + "（笔记里已经写下的旧名字不会自动改）";
    cancelRename();
    await refresh();
  } catch (error) {
    problem.value = String(error);
  }
}

/**
 * 另存为：把仓库里那份拷到用户选的位置。
 *
 * 与笔记里图片右键的那一项走同一条命令（`export_file` 认标识也认显示名），
 * 所以两处的行为不会分家。
 */
async function saveOut(file: FileEntry) {
  problem.value = "";
  try {
    const target = await save({ defaultPath: file.name, title: "另存为" });
    if (!target) {
      return;
    }
    await invoke("export_file", { key: file.id, target });
    notice.value = "已另存为：" + target;
  } catch (error) {
    problem.value = String(error);
  }
}

async function copyReference(file: FileEntry) {
  try {
    await writeText(referenceOf(file));
    notice.value = "已复制引用：" + referenceOf(file);
  } catch (error) {
    problem.value = "复制失败：" + String(error);
  }
}

/** 文件行上的右键：常用动作都在这里，免得每次都要把鼠标挪到右边的按钮上 */
function onRowMenu(event: MouseEvent, file: FileEntry) {
  event.preventDefault();
  event.stopPropagation();
  openMenu(event, [
    { label: "复制引用", run: () => copyReference(file) },
    { label: "重命名", run: () => beginRename(file) },
    { label: "另存为…", run: () => saveOut(file) },
    { label: "删除", danger: true, run: () => askDelete(file) },
  ]);
}

/** 删除前先进入"等确认"状态：右键点删除也不该立刻删掉 */
function askDelete(file: FileEntry) {
  confirming.value = file.id;
}

async function remove(file: FileEntry) {
  problem.value = "";
  try {
    await invoke("delete_file", { id: file.id });
    notice.value = "已删除：" + file.name;
    confirming.value = "";
    await refresh();
  } catch (error) {
    problem.value = String(error);
  }
}

onMounted(() => {
  void refresh();
  window.addEventListener("paste", onPaste);
});

onBeforeUnmount(() => window.removeEventListener("paste", onPaste));
</script>

<template>
  <section class="files">
    <header class="files__head">
      <div class="files__lead">
        <FolderOpen :size="20" />
        <span>{{ files.length }} 个文件</span>
      </div>
      <button class="files__upload" :disabled="busy" @click="upload">
        <Upload :size="16" />
        {{ busy ? "上传中…" : "上传文件" }}
      </button>
    </header>

    <p class="files__hint">
      点「上传文件」选本机文件，或者直接在**这一页**按 Ctrl+V 粘贴截图。
      文件存在仓库的 files/ 目录里，笔记中用文件名引用：图片写
      <code>![名字](名字)</code>；要对齐或限宽，写
      <code>::image src=名字 align=right width=320</code>。
    </p>

    <p v-if="notice" class="files__notice">{{ notice }}</p>
    <p v-if="problem" class="files__problem">{{ problem }}</p>

    <p v-if="files.length === 0" class="files__empty">
      还没有文件。点「上传文件」选一个，它就会出现在这里。
    </p>

    <ul v-else class="files__list">
      <li
        v-for="file in files"
        :key="file.id"
        class="files__item"
        @contextmenu="onRowMenu($event, file)"
      >
        <div class="files__thumb">
          <img v-if="isImage(file)" :src="file.url" :alt="file.name" />
          <span v-else class="files__ext">{{ file.name.split(".").pop() }}</span>
        </div>

        <div class="files__meta">
          <input
            v-if="renaming === file.id"
            v-model="renameText"
            class="files__rename"
            type="text"
            aria-label="新的文件名"
            @keydown.enter.prevent="submitRename(file)"
            @keydown.esc.prevent="cancelRename"
          />
          <p v-else class="files__name">{{ file.name }}</p>
          <p class="files__sub">
            {{ formatSize(file.size) }} · {{ file.mime }} · {{ file.uploaded }}
          </p>
          <code class="files__ref">{{ referenceOf(file) }}</code>
        </div>

        <div class="files__actions">
          <template v-if="renaming === file.id">
            <button @click="submitRename(file)">确认改名</button>
            <button @click="cancelRename">取消</button>
          </template>
          <template v-else-if="confirming === file.id">
            <button class="files__danger" @click="remove(file)">确认删除</button>
            <button @click="confirming = ''">取消</button>
          </template>
          <template v-else>
            <button @click="copyReference(file)">复制引用</button>
            <button @click="beginRename(file)">重命名</button>
            <button class="files__danger" @click="askDelete(file)">
              <Trash2 :size="15" />
              删除
            </button>
          </template>
        </div>
      </li>
    </ul>
  </section>
</template>

<style scoped>
.files {
  padding: 4px 0 32px;
}

.files__head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 8px;
}

.files__lead {
  display: flex;
  align-items: center;
  gap: 8px;
  color: var(--text-dim);
}

.files__upload {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 6px 12px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: var(--accent-solid);
  color: var(--text);
  cursor: pointer;
}

.files__upload:disabled {
  opacity: 0.6;
  cursor: default;
}

.files__hint {
  margin: 0 0 16px;
  color: var(--text-dim);
  font-size: 0.92em;
  line-height: 1.7;
}

.files__hint code {
  padding: 1px 5px;
  border-radius: 4px;
  background: var(--surface);
}

.files__notice {
  margin: 0 0 12px;
  color: var(--text-dim);
}

.files__problem {
  margin: 0 0 12px;
  color: var(--danger);
}

.files__empty {
  color: var(--text-dim);
}

.files__list {
  margin: 0;
  padding: 0;
  list-style: none;
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.files__item {
  display: flex;
  align-items: center;
  gap: 14px;
  padding: 10px 12px;
  border: 1px solid var(--border);
  border-radius: 8px;
  background: var(--surface);
}

.files__thumb {
  flex: none;
  width: 64px;
  height: 64px;
  display: grid;
  place-items: center;
  overflow: hidden;
  border-radius: 6px;
  background: var(--bg);
}

.files__thumb img {
  max-width: 100%;
  max-height: 100%;
}

.files__ext {
  color: var(--text-dim);
  font-size: 0.8em;
  text-transform: uppercase;
}

.files__meta {
  flex: 1;
  min-width: 0;
}

.files__name {
  margin: 0 0 4px;
  font-weight: 600;
  word-break: break-all;
}

.files__rename {
  width: 100%;
  margin: 0 0 4px;
  padding: 5px 8px;
  border: 1px solid var(--accent);
  border-radius: 6px;
  background: var(--bg);
  color: var(--text);
  font-size: 1rem;
  font-weight: 600;
}

.files__sub {
  margin: 0 0 6px;
  color: var(--text-dim);
  font-size: 0.85em;
}

.files__ref {
  color: var(--text-dim);
  font-size: 0.85em;
  word-break: break-all;
}

.files__actions {
  flex: none;
  display: flex;
  align-items: center;
  gap: 6px;
}

.files__actions button {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 5px 10px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: var(--bg);
  color: var(--text);
  cursor: pointer;
}

/* 红色只给破坏性操作 */
.files__danger {
  color: var(--danger);
  border-color: var(--danger);
}
</style>
