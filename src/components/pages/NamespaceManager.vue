<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { Plus } from "@lucide/vue";
import type { Namespace } from "../../bindings/namespace.ts";

/**
 * 命名空间管理（设置页的一节，地址 `special:settings#namespaces`）。
 *
 * 三件事：**建**（表尾的表单）、**改**（行内编辑：别名、站址、改名）、**清 / 删**
 * （两步确认 —— 这两件事都会把页面移进回收站）。
 *
 * 命令一律返回**更新后的整张表**，这里直接替换，不必再拉一次；
 * 校验的权威在后端（名字重复、保留名、非法字符都是那边说了算），这里只挡明显的空输入。
 */

const MAIN = "0";
const SPECIAL = "special";
const TEMPLATE = "template";

const items = ref<Namespace[]>([]);
const busy = ref(false);
const error = ref("");
const loading = ref(true);

/** 正在行内编辑哪一行、编哪个字段 */
const editing = ref<{ id: string; field: "aliases" | "site" | "rename" } | null>(null);
/** 别名编辑的草稿（tags） */
const aliasDraft = ref<string[]>([]);
const aliasInput = ref("");
const siteDraft = ref("");
const nameDraft = ref("");

/** 两道确认的破坏性操作：`清空:<标识>` / `删除:<标识>` */
const armed = ref("");

/** 新建表单 */
const newName = ref("");
const newSite = ref("");
const newKind = ref<"internal" | "external">("internal");

const canCreate = computed(
  () =>
    !busy.value &&
    newName.value.trim().length > 0 &&
    (newKind.value === "internal" || newSite.value.trim().length > 0),
);

const reserved = (item: Namespace) =>
  item.id === MAIN || item.id === SPECIAL || item.id === TEMPLATE;

function of(item: Namespace): string {
  return item.name || "（主）";
}

function kindOf(item: Namespace): string {
  if (item.id === MAIN) {
    return "主命名空间";
  }
  if (item.id === SPECIAL) {
    return "虚拟命名空间（页面由程序提供）";
  }
  if (item.site) {
    return "跨站命名空间";
  }
  return item.id === TEMPLATE ? "保留命名空间（模板与样式）" : "内容命名空间";
}

/** 锚点：`special:settings#ns-help` 能定位到这一行 */
function anchorOf(item: Namespace): string {
  if (item.id === MAIN) {
    return "ns-main";
  }
  if (item.id === SPECIAL) {
    return "ns-special";
  }
  return `ns-${item.name.trim().replace(/\s+/g, "-") || item.id}`;
}

async function load() {
  loading.value = true;
  try {
    items.value = await invoke<Namespace[]>("namespaces");
  } catch (reason) {
    error.value = String(reason);
  } finally {
    loading.value = false;
  }
}

onMounted(() => void load());

/** 所有写操作都从这里过：一步到位地把"忙/错/刷新/收摊"处理干净 */
async function act(command: string, args: Record<string, unknown>) {
  busy.value = true;
  error.value = "";
  try {
    items.value = await invoke<Namespace[]>(command, args);
    editing.value = null;
    armed.value = "";
    return true;
  } catch (reason) {
    error.value = String(reason);
    return false;
  } finally {
    busy.value = false;
  }
}

// ---------------- 行内编辑 ----------------

function startAliases(item: Namespace) {
  editing.value = { id: item.id, field: "aliases" };
  aliasDraft.value = [...item.aliases];
  aliasInput.value = "";
}

function addAlias() {
  const alias = aliasInput.value.trim();
  if (alias && !aliasDraft.value.some((kept) => kept.toLowerCase() === alias.toLowerCase())) {
    aliasDraft.value.push(alias);
  }
  aliasInput.value = "";
}

function dropAlias(index: number) {
  aliasDraft.value.splice(index, 1);
}

function saveAliases(item: Namespace) {
  void act("update_namespace", {
    key: item.id,
    aliases: aliasDraft.value,
    site: item.site,
  });
}

function startSite(item: Namespace) {
  editing.value = { id: item.id, field: "site" };
  siteDraft.value = item.site ?? "";
}

function saveSite(item: Namespace) {
  void act("update_namespace", {
    key: item.id,
    aliases: item.aliases,
    site: siteDraft.value.trim() || null,
  });
}

function startRename(item: Namespace) {
  editing.value = { id: item.id, field: "rename" };
  nameDraft.value = item.name;
}

function saveRename(item: Namespace) {
  const name = nameDraft.value.trim();
  if (!name || name === item.name) {
    editing.value = null;
    return;
  }
  void act("rename_namespace", { key: item.id, name });
}

// ---------------- 清空 / 删除（两步确认） ----------------

function emptyNamespace(item: Namespace) {
  if (armed.value !== `empty:${item.id}`) {
    armed.value = `empty:${item.id}`;
    return;
  }
  void act("empty_namespace", { key: item.id });
}

function deleteNamespace(item: Namespace) {
  if (armed.value !== `delete:${item.id}`) {
    armed.value = `delete:${item.id}`;
    return;
  }
  void act("delete_namespace", { key: item.id });
}

// ---------------- 新建 ----------------

function create() {
  if (!canCreate.value) {
    return;
  }
  void act("add_namespace", {
    name: newName.value.trim(),
    aliases: [],
    site: newKind.value === "external" ? newSite.value.trim() || null : null,
  }).then((ok) => {
    if (ok) {
      newName.value = "";
      newSite.value = "";
      newKind.value = "internal";
    }
  });
}
</script>

<template>
  <section class="ns">
    <p class="ns__lead">
      命名空间是标题的前缀（`帮助:入门`）。<strong>标识与名称分开存放</strong>，
      所以改名不会移动任何文件。配了站点地址的命名空间用来写跨站链接
      （`[[zhwiki:条目]]` 画成绿链，交给浏览器打开），页面不在本仓库。
    </p>

    <p v-if="error" class="ns__notice">{{ error }}</p>
    <p v-if="loading" class="ns__hint">正在读…</p>

    <div v-else class="ns__table">
      <div v-for="item in items" :key="item.id" :id="anchorOf(item)" class="ns__row">
        <div class="ns__head">
          <span class="ns__name">{{ of(item) }}</span>
          <code class="ns__id">{{ item.id }}</code>
          <span class="ns__kind">{{ kindOf(item) }}</span>
        </div>

        <!-- 别名 -->
        <div v-if="editing?.id === item.id && editing.field === 'aliases'" class="ns__edit">
          <span v-for="(alias, index) in aliasDraft" :key="alias" class="ns__tag">
            {{ alias }}
            <button type="button" title="去掉这个别名" @click="dropAlias(index)">×</button>
          </span>
          <input
              v-model="aliasInput"
              class="ns__input"
              type="text"
              placeholder="新别名"
              @keydown.enter.prevent="addAlias"
          />
          <button type="button" class="ns__btn" @click="addAlias">添加</button>
          <button type="button" class="ns__btn ns__btn--go" :disabled="busy" @click="saveAliases(item)">
            保存
          </button>
          <button type="button" class="ns__btn" @click="editing = null">取消</button>
        </div>
        <div v-else class="ns__line">
          <span class="ns__label">别名</span>
          <span v-if="item.aliases.length === 0" class="ns__dim">—</span>
          <span v-for="alias in item.aliases" :key="alias" class="ns__tag">{{ alias }}</span>
          <button type="button" class="ns__link" @click="startAliases(item)">编辑</button>
        </div>

        <!-- 站点地址 -->
        <div v-if="editing?.id === item.id && editing.field === 'site'" class="ns__edit">
          <input
              v-model="siteDraft"
              class="ns__input ns__input--wide"
              type="text"
              placeholder="站点地址模板，页面名以 $1 占位"
              @keydown.enter.prevent="saveSite(item)"
          />
          <button type="button" class="ns__btn ns__btn--go" :disabled="busy" @click="saveSite(item)">
            保存
          </button>
          <button type="button" class="ns__btn" @click="editing = null">取消</button>
        </div>
        <div v-else-if="!reserved(item)" class="ns__line">
          <span class="ns__label">站址</span>
          <code v-if="item.site" class="ns__site">{{ item.site }}</code>
          <span v-else class="ns__dim">—</span>
          <button type="button" class="ns__link" @click="startSite(item)">编辑</button>
        </div>

        <!-- 改名 -->
        <div v-if="editing?.id === item.id && editing.field === 'rename'" class="ns__edit">
          <input
              v-model="nameDraft"
              class="ns__input"
              type="text"
              placeholder="新名称"
              @keydown.enter.prevent="saveRename(item)"
          />
          <button type="button" class="ns__btn ns__btn--go" :disabled="busy" @click="saveRename(item)">
            确定
          </button>
          <button type="button" class="ns__btn" @click="editing = null">取消</button>
        </div>

        <div class="ns__actions">
          <button
              v-if="!reserved(item)"
              type="button"
              class="ns__btn"
              :disabled="busy"
              @click="startRename(item)"
          >
            改名
          </button>
          <button
              v-if="item.id !== SPECIAL"
              type="button"
              class="ns__btn"
              :disabled="busy"
              @click="emptyNamespace(item)"
          >
            {{ armed === `empty:${item.id}` ? "确认清空" : "清空" }}
          </button>
          <button
              v-if="!reserved(item)"
              type="button"
              class="ns__btn ns__btn--danger"
              :disabled="busy"
              @click="deleteNamespace(item)"
          >
            {{ armed === `delete:${item.id}` ? "确认删除" : "删除" }}
          </button>
          <span v-if="armed.startsWith(`empty:${item.id}`) || armed.startsWith(`delete:${item.id}`)" class="ns__warn">
            里面的页面会移进回收站（还能还原）
          </span>
        </div>
      </div>
    </div>

    <div class="ns__create">
      <span class="ns__label">新建</span>
      <input v-model="newName" class="ns__input" type="text" placeholder="名称"/>
      <select v-model="newKind" class="ns__input">
        <option value="internal">内容命名空间</option>
        <option value="external">跨站命名空间</option>
      </select>
      <input
          v-if="newKind === 'external'"
          v-model="newSite"
          class="ns__input ns__input--wide"
          type="text"
          placeholder="站点地址模板，页面名以 $1 占位"
      />
      <button type="button" class="ns__btn ns__btn--go" :disabled="!canCreate" @click="create">
        <Plus :size="14" :stroke-width="2"/>
        创建
      </button>
    </div>

    <p class="ns__hint">
      名称与别名不得重复（忽略大小写与空白），也不能含 `/` `:` `@` `#` 等字符。
      别名只是"也认这个写法"：回显与列表里用的始终是规范名。
    </p>
  </section>
</template>

<style scoped>
.ns {
  margin: 4px 0 0;
}

.ns__lead {
  margin: 0 0 12px;
  color: var(--text-dim);
  font-size: 13px;
  line-height: 1.75;
}

.ns__notice {
  margin: 0 0 10px;
  padding: 8px 12px;
  border: 1px solid var(--danger);
  border-left-width: 3px;
  border-radius: 8px;
  color: var(--text);
  font-size: 13px;
}

.ns__hint {
  margin: 10px 0 0;
  color: var(--text-dim);
  font-size: 12.5px;
  line-height: 1.7;
}

.ns__table {
  border: 1px solid var(--border);
  border-radius: 8px;
  overflow: hidden;
}

.ns__row {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 10px 12px;
  border-top: 1px solid var(--border);
  /* 从地址的 `#ns-xxx` 跳过来时别贴着顶边 */
  scroll-margin-top: 24px;
}

.ns__row:first-child {
  border-top: 0;
}

.ns__row:nth-child(odd) {
  background: var(--surface);
}

.ns__head {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  align-items: baseline;
}

.ns__name {
  font-size: 13.5px;
  font-weight: 600;
}

.ns__id,
.ns__site {
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

.ns__kind {
  color: var(--text-dim);
  font-size: 12px;
}

.ns__line,
.ns__edit,
.ns__create,
.ns__actions {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  align-items: center;
}

.ns__label {
  min-width: 34px;
  color: var(--text-dim);
  font-size: 12px;
}

.ns__dim {
  color: var(--text-dim);
  font-size: 12.5px;
}

.ns__tag {
  display: inline-flex;
  gap: 4px;
  align-items: center;
  padding: 1px 8px;
  border: 1px solid var(--border);
  border-radius: 999px;
  color: var(--text-dim);
  font-size: 12px;
}

.ns__tag button {
  padding: 0;
  border: 0;
  background: transparent;
  color: var(--text-dim);
  font: inherit;
  cursor: pointer;
}

.ns__tag button:hover {
  color: var(--danger);
}

.ns__input {
  height: 26px;
  padding: 0 8px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: var(--field-bg);
  color: var(--text);
  font: inherit;
  font-size: 12.5px;
}

.ns__input--wide {
  flex: 1 1 220px;
  min-width: 160px;
}

.ns__input:focus {
  outline: none;
  border-color: var(--accent-soft);
}

.ns__btn {
  display: inline-flex;
  gap: 4px;
  align-items: center;
  padding: 4px 10px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: transparent;
  color: var(--text-dim);
  font: inherit;
  font-size: 12.5px;
  cursor: pointer;
}

.ns__btn:hover:not(:disabled) {
  background: var(--hover);
  color: var(--text);
}

.ns__btn:disabled {
  opacity: 0.5;
  cursor: default;
}

.ns__btn--go {
  border-color: var(--accent-soft);
  color: var(--accent-soft);
}

.ns__btn--go:hover:not(:disabled) {
  background: var(--accent);
  color: var(--text);
}

.ns__btn--danger:hover:not(:disabled) {
  border-color: var(--danger);
  color: var(--danger);
  background: transparent;
}

/* 行内的"编辑"是个纯文字入口：它不改数据，所以不必长成按钮 */
.ns__link {
  padding: 0;
  border: 0;
  background: transparent;
  color: var(--accent-soft);
  font: inherit;
  font-size: 12px;
  cursor: pointer;
}

.ns__link:hover {
  text-decoration: underline;
}

.ns__warn {
  color: var(--text-dim);
  font-size: 12px;
}

.ns__create {
  margin-top: 12px;
}
</style>
