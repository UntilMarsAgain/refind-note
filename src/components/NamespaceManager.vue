<script setup lang="ts">
/**
 * 命名空间管理。
 *
 * 「有哪些命名空间」**以后端为准**（`namespaces` 命令）：这里只负责显示与提交；
 * 重名、非法字符、保留名这些校验**全在后端** —— 前端不重复判断，免得两处规则迟早不一致。
 *
 * 两个始终存在、删不掉的：主命名空间（显示成「（主）」）与虚拟的 `special`。
 *
 * 刻意不用 `window.prompt` / `window.confirm`：在系统 WebView 里不可靠。改名用行内输入，
 * 清空与删除用**两步确认**（按钮先变成"确认…"，再点一次才真的执行）。
 */
import { onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";

interface Namespace {
  id: string;
  name: string;
  aliases: string[];
  /** 可存储：页面落在本仓库；虚拟与跨站的都不是 */
  storable: boolean;
  /** 跨站链接的站点地址模板；null = 普通命名空间 */
  site: string | null;
}

const MAIN = "0";
const SPECIAL = "special";

const items = ref<Namespace[]>([]);
const error = ref("");
const loading = ref(true);
/** 正在处理哪一项（阻止连点，也让用户看到在处理谁） */
const busy = ref("");
/** 改名中的那一项，以及它的输入 */
const editingId = ref("");
const editingName = ref("");
/** 已按过一次"确认"的破坏性操作（key = `清空:id` / `删除:id`） */
const armed = ref("");

const newName = ref("");
const newAliases = ref("");
const newSite = ref("");

function labelOf(item: Namespace): string {
  return item.name || "（主）";
}

/** 主命名空间与 special：不能改名、不能清空、不能删除 */
function isReserved(item: Namespace): boolean {
  return item.id === MAIN || item.id === SPECIAL;
}

function kindOf(item: Namespace): string {
  if (item.id === MAIN) {
    return "主命名空间";
  }
  if (item.id === SPECIAL) {
    return "虚拟命名空间（页面由程序提供）";
  }
  return item.site ? "跨站命名空间" : "内容命名空间";
}

async function refresh() {
  items.value = await invoke<Namespace[]>("namespaces");
}

async function act(key: string, action: () => Promise<Namespace[]>) {
  busy.value = key;
  error.value = "";
  armed.value = "";
  try {
    items.value = await action();
  } catch (reason) {
    error.value = String(reason);
  } finally {
    busy.value = "";
  }
}

onMounted(async () => {
  try {
    await refresh();
  } catch (reason) {
    error.value = String(reason);
  } finally {
    loading.value = false;
  }
});

function parseAliases(text: string): string[] {
  return text
    .split(/[,，\s]+/)
    .map((alias) => alias.trim())
    .filter((alias) => alias.length > 0);
}

async function add() {
  await act("add", async () => {
    const next = await invoke<Namespace[]>("add_namespace", {
      name: newName.value,
      aliases: parseAliases(newAliases.value),
      site: newSite.value.trim() || null,
    });
    newName.value = "";
    newAliases.value = "";
    newSite.value = "";
    return next;
  });
}

function startRename(item: Namespace) {
  editingId.value = item.id;
  editingName.value = item.name;
  armed.value = "";
}

async function submitRename(item: Namespace) {
  const name = editingName.value.trim();
  editingId.value = "";
  if (!name || name === item.name) {
    return;
  }
  await act(item.id, () =>
    invoke<Namespace[]>("rename_namespace", { key: item.id, name }),
  );
}

function empty(item: Namespace) {
  const key = `清空:${item.id}`;
  if (armed.value !== key) {
    armed.value = key;
    return;
  }
  void act(item.id, () =>
    invoke<Namespace[]>("empty_namespace", { key: item.id }),
  );
}

function remove(item: Namespace) {
  const key = `删除:${item.id}`;
  if (armed.value !== key) {
    armed.value = key;
    return;
  }
  void act(item.id, () =>
    invoke<Namespace[]>("delete_namespace", { key: item.id }),
  );
}
</script>

<template>
  <div class="ns">
    <p class="ns__lead">
      命名空间用名称作为前缀（<code>帮助:入门</code>）。标识与名称分开：**改名不会移动任何文件**。
      配了站点地址的就是跨站命名空间，<code>[[zhwiki:NASA]]</code> 会渲染成绿链。
    </p>

    <p v-if="error" class="ns__error">{{ error }}</p>
    <p v-else-if="loading" class="ns__hint">正在读取…</p>

    <ul v-else class="ns__list">
      <li v-for="item in items" :key="item.id" class="ns__item">
        <div class="ns__head">
          <span class="ns__name">{{ labelOf(item) }}</span>
          <span class="ns__kind">{{ kindOf(item) }}</span>
          <span v-if="item.aliases.length" class="ns__aliases">
            别名：{{ item.aliases.join("、") }}
          </span>
          <span v-if="item.site" class="ns__site">{{ item.site }}</span>
        </div>

        <div class="ns__actions">
          <template v-if="editingId === item.id">
            <input
              v-model="editingName"
              class="ns__input"
              type="text"
              @keydown.enter.prevent="submitRename(item)"
              @keydown.esc="editingId = ''"
            />
            <button class="ns__btn" type="button" @click="submitRename(item)">确定</button>
            <button class="ns__btn" type="button" @click="editingId = ''">取消</button>
          </template>

          <template v-else-if="!isReserved(item)">
            <button
              class="ns__btn"
              type="button"
              :disabled="busy === item.id"
              @click="startRename(item)"
            >
              改名
            </button>
            <button
              class="ns__btn"
              type="button"
              :disabled="busy === item.id"
              @click="empty(item)"
            >
              {{ armed === `清空:${item.id}` ? "确认清空" : "清空" }}
            </button>
            <button
              class="ns__btn ns__btn--danger"
              type="button"
              :disabled="busy === item.id"
              @click="remove(item)"
            >
              {{ armed === `删除:${item.id}` ? "确认删除" : "删除" }}
            </button>
          </template>

          <span v-else class="ns__locked">不可改名 / 清空 / 删除</span>
        </div>
      </li>
    </ul>

    <div class="ns__add">
      <input v-model="newName" class="ns__input" type="text" placeholder="名称（如 help）" />
      <input
        v-model="newAliases"
        class="ns__input"
        type="text"
        placeholder="别名，逗号分隔（可留空）"
      />
      <input
        v-model="newSite"
        class="ns__input ns__input--wide"
        type="text"
        placeholder="跨站地址模板，可留空（如 https://zh.wikipedia.org/wiki/$1）"
      />
      <button
        class="ns__btn ns__btn--primary"
        type="button"
        :disabled="busy === 'add' || !newName.trim()"
        @click="add"
      >
        添加命名空间
      </button>
    </div>
    <p class="ns__hint">
      名称与别名都不许重复（大小写与空白不影响判重），也不许有 <code>/</code> <code>:</code>
      <code>@</code> <code>#</code> 等符号。清空与删除都会把页面送进回收站。
    </p>
  </div>
</template>

<style scoped>
.ns__lead,
.ns__hint {
  margin: 0 0 12px;
  color: var(--text-dim);
  font-size: 12.5px;
  line-height: 1.7;
}

.ns__lead code,
.ns__hint code {
  padding: 1px 5px;
  border-radius: 4px;
  background: var(--code-bg);
  font-size: 12px;
}

.ns__list {
  margin: 0 0 16px;
  padding: 0;
  list-style: none;
}

.ns__item {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 10px;
  padding: 9px 0;
  border-bottom: 1px solid var(--border);
}

.ns__head {
  display: flex;
  align-items: baseline;
  flex-wrap: wrap;
  gap: 8px;
  flex: 1 1 auto;
  min-width: 0;
}

.ns__name {
  font-size: 14px;
}

.ns__kind,
.ns__aliases,
.ns__site {
  color: var(--text-dim);
  font-size: 12px;
}

.ns__site {
  font-family: var(--mono, monospace);
}

.ns__actions {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-shrink: 0;
}

.ns__locked {
  color: var(--text-dim);
  font-size: 12px;
}

.ns__btn {
  padding: 3px 10px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background-color: transparent;
  color: var(--text);
  font-size: 12px;
  cursor: pointer;
}

.ns__btn:hover:not(:disabled) {
  background-color: var(--hover);
}

.ns__btn:disabled {
  opacity: 0.45;
  cursor: default;
}

.ns__btn--danger {
  color: var(--link-missing);
}

.ns__btn--primary {
  color: var(--accent-soft);
}

.ns__input {
  width: 130px;
  padding: 3px 8px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: var(--field-bg);
  color: var(--text);
  font-size: 12.5px;
}

.ns__input--wide {
  width: 300px;
  max-width: 100%;
}

.ns__add {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 6px;
  margin-bottom: 8px;
}

.ns__error {
  color: var(--link-missing);
  font-size: 13px;
}
</style>
