<script setup lang="ts">
import type { Namespace } from "../bindings";
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
 *
 * 注意：模板里**不要用反引号模板字符串** —— Vue 的模板表达式解析不了它们，整个模板会
 * 绑定失败（本项目在 NoteEditor 上踩过一次，这次又踩了一次）。要拼接就用 `+`。
 */
import { computed, onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";


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
/** 已按过一次"确认"的破坏性操作（`清空:<标识>` / `删除:<标识>`） */
const armed = ref("");
/** 正在编辑别名的那一项，以及它的草稿（点保存才落盘） */
const aliasId = ref("");
const aliasDraft = ref<string[]>([]);
const aliasInput = ref("");

const newName = ref("");
const newSite = ref("");
/** 内部（内容命名空间）还是外部（跨站）：用选择框明确下来，而不是让用户猜那个地址栏要不要填 */
const newKind = ref<"internal" | "external">("internal");

/** 名称必填；选跨站时还要填站点地址模板 */
const canCreate = computed(
  () =>
    busy.value !== "add" &&
    newName.value.trim().length > 0 &&
    (newKind.value === "internal" || newSite.value.trim().length > 0),
);

function labelOf(item: Namespace): string {
  return item.name || "（主）";
}

/** 主命名空间与 special：不能改名、不能清空、不能删除 */
function isReserved(item: Namespace): boolean {
  return item.id === MAIN || item.id === SPECIAL;
}

/**
 * 每一项的**可导航 id**：`ns-<名称>`（空白折成 `-`）。
 *
 * 设置项各有 id，命名空间也该有 —— 于是 `special:settings#ns-help` 能直接跳到它。
 */
function anchorOf(item: Namespace): string {
  if (item.id === MAIN) {
    return "ns-main";
  }
  if (item.id === SPECIAL) {
    return "ns-special";
  }
  return "ns-" + item.name.trim().replace(/\s+/g, "-");
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

/**
 * 新建命名空间：**只带名称**（外加可选的跨站地址）。
 *
 * 别名不在这里加 —— 那是另一件事，建好之后在列表里点「别名」编辑。两件事挤在同一个
 * 表单里，只会让人以为"必须一次填完"。
 */
async function add() {
  await act("add", async () => {
    const next = await invoke<Namespace[]>("add_namespace", {
      name: newName.value,
      aliases: [],
      site: newKind.value === "external" ? newSite.value.trim() || null : null,
    });
    newName.value = "";
    newSite.value = "";
    newKind.value = "internal";
    return next;
  });
}

/**
 * 谁可以清空：有页面文件的都能 —— **主命名空间也可以**（你要求的）。
 * `special` 是虚拟的，页面由程序提供，没有可清的页面。
 */
function canEmpty(item: Namespace): boolean {
  return item.id !== SPECIAL;
}

function startAliases(item: Namespace) {
  aliasId.value = item.id;
  aliasDraft.value = [...item.aliases];
  aliasInput.value = "";
  armed.value = "";
}

function addAlias() {
  const value = aliasInput.value.trim();
  if (value && !aliasDraft.value.includes(value)) {
    aliasDraft.value = [...aliasDraft.value, value];
  }
  aliasInput.value = "";
}

function dropAlias(alias: string) {
  aliasDraft.value = aliasDraft.value.filter((value) => value !== alias);
}

async function saveAliases(item: Namespace) {
  await act(item.id, () =>
    invoke<Namespace[]>("update_namespace_aliases", {
      key: item.id,
      aliases: aliasDraft.value,
    }),
  );
  aliasId.value = "";
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
  const key = "清空:" + item.id;
  if (armed.value !== key) {
    armed.value = key;
    return;
  }
  void act(item.id, () =>
    invoke<Namespace[]>("empty_namespace", { key: item.id }),
  );
}

function remove(item: Namespace) {
  const key = "删除:" + item.id;
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
      命名空间是标题的前缀。<strong>标识与名称分开存放</strong>，因此改名不会移动任何文件。
      配置了站点地址的命名空间用于跨站链接，其类型为外部。
    </p>

    <p v-if="loading" class="ns__hint">正在读取…</p>

    <template v-else>

      <div class="ns__table">
        <div class="ns__head-row" aria-hidden="true">
          <span>名称</span>
          <span>标识</span>
          <span>别名</span>
          <span>类型 / 跨站地址</span>
          <span class="ns__head-actions">操作</span>
        </div>

        <div
          v-for="item in items"
          :id="anchorOf(item)"
          :key="item.id"
          class="ns__item"
          :title="'地址锚点：#' + anchorOf(item)"
        >
          <div class="ns__cell">
            <span class="ns__name">{{ labelOf(item) }}</span>
          </div>

          <!-- 标识单独一列：它是磁盘上真正用的键，与名称不是一回事 -->
          <div class="ns__cell">
            <code class="ns__id">{{ item.id }}</code>
          </div>

          <div class="ns__cell">
            <span v-if="item.aliases.length" class="ns__chips">
              <span v-for="alias in item.aliases" :key="alias" class="ns__chip">
                {{ alias }}
              </span>
            </span>
            <span v-else class="ns__dim">—</span>
          </div>

          <div class="ns__cell">
            <span class="ns__kind">{{ kindOf(item) }}</span>
            <code v-if="item.site" class="ns__site">{{ item.site }}</code>
          </div>

          <div class="ns__cell ns__cell--actions">
            <!-- 别名编辑：草稿 + 保存（点保存才落盘，避免每敲一下就发一次请求） -->
            <template v-if="aliasId === item.id">
              <span class="ns__chips">
                <span v-for="alias in aliasDraft" :key="alias" class="ns__chip">
                  {{ alias }}
                  <button
                    class="ns__chip-x"
                    type="button"
                    :aria-label="`删除别名 ${alias}`"
                    @click="dropAlias(alias)"
                  >
                    ×
                  </button>
                </span>
                <span v-if="!aliasDraft.length" class="ns__dim">还没有别名</span>
              </span>
              <input
                v-model="aliasInput"
                class="ns__input"
                type="text"
                placeholder="新别名"
                @keydown.enter.prevent="addAlias"
              />
              <button class="ns__btn" type="button" @click="addAlias">添加</button>
              <button class="ns__btn ns__btn--primary" type="button" @click="saveAliases(item)">
                保存
              </button>
              <button class="ns__btn" type="button" @click="aliasId = ''">取消</button>
            </template>

            <template v-else-if="editingId === item.id">
              <input
                v-model="editingName"
                class="ns__input"
                type="text"
                placeholder="新名称"
                @keydown.enter.prevent="submitRename(item)"
                @keydown.esc="editingId = ''"
              />
              <button class="ns__btn" type="button" @click="submitRename(item)">确定</button>
              <button class="ns__btn" type="button" @click="editingId = ''">取消</button>
            </template>

            <template v-else>
              <button
                class="ns__btn"
                type="button"
                :disabled="busy === item.id"
                @click="startAliases(item)"
              >
                别名
              </button>
              <!-- 主命名空间只不能改名与删除；special 只能配别名 -->
              <button
                v-if="!isReserved(item)"
                class="ns__btn"
                type="button"
                :disabled="busy === item.id"
                @click="startRename(item)"
              >
                改名
              </button>
              <!-- 清空与删除都会改数据：清空把页面移入回收站（可还原），
                   删除连命名空间本身也不再保留。两者都用危险色标出来。 -->
              <button
                v-if="canEmpty(item)"
                class="ns__btn ns__btn--danger"
                type="button"
                :disabled="busy === item.id"
                @click="empty(item)"
              >
                {{ armed === '清空:' + item.id ? '确认清空' : '清空' }}
              </button>
              <button
                v-if="!isReserved(item)"
                class="ns__btn ns__btn--danger"
                type="button"
                :disabled="busy === item.id"
                @click="remove(item)"
              >
                {{ armed === '删除:' + item.id ? '确认删除' : '删除' }}
              </button>
            </template>
          </div>
        </div>
      </div>

      <!-- 出错只报错，**不顶掉列表与创建区** —— 以前它挤在 v-else 的位置上，
           一旦创建失败，整个内容跟着消失，用户连改哪儿都看不到。 -->
      <p v-if="error" class="ns__notice">
        <strong>操作未完成</strong>
        <span>{{ error }}</span>
      </p>

      <section class="ns__create">
        <h3 class="ns__create-title">新建命名空间</h3>
        <p class="ns__create-lead">
          创建内容命名空间，或指向其它站点的跨站命名空间。别名在创建之后于列表中编辑。
        </p>

        <div class="ns__create-fields">
          <label class="ns__field">
            <span>名称</span>
            <input
              v-model="newName"
              class="ns__input"
              type="text"
              @keydown.enter.prevent="add"
            />
          </label>

          <label class="ns__field">
            <span>类型</span>
            <select v-model="newKind" class="ns__input">
              <option value="internal">内容命名空间</option>
              <option value="external">跨站命名空间</option>
            </select>
          </label>

          <label v-if="newKind === 'external'" class="ns__field ns__field--wide">
            <span>站点地址模板（页面名以 $1 占位）</span>
            <input
              v-model="newSite"
              class="ns__input"
              type="text"
              @keydown.enter.prevent="add"
            />
          </label>

          <button
            class="ns__btn ns__btn--primary ns__btn--big"
            type="button"
            :disabled="!canCreate"
            @click="add"
          >
            创建
          </button>
        </div>

        <p class="ns__hint">
          名称与别名不得重复（忽略大小写与空白），且不得包含
          <code>/</code> <code>:</code> <code>@</code> <code>#</code> 等字符。
          清空与删除都会把页面移入回收站。
        </p>
      </section>
    </template>
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

.ns__cell--actions .ns__chips {
  max-width: 320px;
}

.ns__lead code,
.ns__hint code {
  padding: 1px 5px;
  border-radius: 4px;
  background: var(--code-bg);
  font-size: 12px;
}

/* ---------- 一张表：表头 + 每行四格 ---------- */

.ns__table {
  margin-bottom: 18px;
  border: 1px solid var(--border);
  border-radius: 8px;
  overflow: hidden;
}

.ns__head-row,
.ns__item {
  display: grid;
  /* 每一行各自是一个网格，所以最后一列**必须给固定宽度**：
     用 auto 时它的宽度随该行按钮数变化，前几列跟着被挤，各行就对不齐了。 */
  grid-template-columns:
    minmax(84px, 0.9fr)   /* 名称 */
    minmax(56px, 0.5fr)   /* 标识 */
    minmax(84px, 1fr)     /* 别名 */
    minmax(130px, 1.5fr)  /* 类型 / 跨站地址 */
    248px;                /* 操作：够放四个按钮，再宽就交给下面的折行 */
  gap: 8px 14px;
  align-items: center;
  padding: 9px 12px;
}

.ns__head-row {
  border-bottom: 1px solid var(--border);
  background: var(--code-bg);
  color: var(--text-dim);
  font-size: 12px;
}

.ns__head-actions {
  justify-self: end;
}

.ns__item {
  border-bottom: 1px solid var(--border);
  /* 从 `special:settings#ns-help` 跳过来时，别被粘顶的标题压住 */
  scroll-margin-top: 80px;
}

.ns__item:last-child {
  border-bottom: 0;
}

.ns__item:hover {
  background: var(--hover);
}

.ns__cell {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 6px;
  min-width: 0;
}

.ns__cell--actions {
  justify-content: flex-end;
  /* 中等宽度下按钮先在自己这一格内换行，而不是被容器裁掉 */
  flex-wrap: wrap;
}

.ns__name {
  font-size: 13.5px;
  font-weight: 500;
}

.ns__id,
.ns__site {
  padding: 1px 5px;
  border-radius: 4px;
  background: var(--code-bg);
  color: var(--text-dim);
  font-size: 11px;
  overflow-wrap: anywhere;
}

.ns__chips {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
}

.ns__chip-x {
  margin-left: 4px;
  padding: 0;
  border: 0;
  background-color: transparent;
  /* 去掉一个别名不是危险操作，不必用红 */
  color: var(--text-dim);
  font-size: 12px;
  line-height: 1;
  cursor: pointer;
}

.ns__chip {
  padding: 1px 7px;
  border-radius: 9px;
  background: var(--code-bg);
  color: var(--text-dim);
  font-size: 11.5px;
}

.ns__kind,
.ns__dim,
.ns__locked,
.ns__hint {
  color: var(--text-dim);
  font-size: 12px;
}

/* ---------- 新建命名空间：单独一块，别和"别名"混在一起 ---------- */

.ns__create {
  padding: 14px 16px;
  border: 1px solid var(--border);
  border-radius: 8px;
  /* 用**主题色的淡染**（不是 --surface：那个在深色主题里偏红） */
  background: var(--accent-tint);
}

.ns__create-title {
  margin: 0 0 6px;
  font-size: 13.5px;
  font-weight: 500;
}

.ns__create-lead {
  margin: 0 0 12px;
  color: var(--text-dim);
  font-size: 12.5px;
  line-height: 1.7;
}

.ns__create-lead code {
  padding: 1px 5px;
  border-radius: 4px;
  background: var(--code-bg);
  font-size: 12px;
  color: var(--text);
}

.ns__create-fields {
  display: flex;
  align-items: flex-end;
  flex-wrap: wrap;
  gap: 10px;
  margin-bottom: 10px;
}

.ns__field {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.ns__field > span {
  color: var(--text-dim);
  font-size: 11.5px;
}

.ns__field--wide .ns__input {
  width: 320px;
  max-width: 100%;
}

.ns__input {
  width: 140px;
  padding: 4px 8px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: var(--field-bg);
  color: var(--text);
  font-size: 12.5px;
}

.ns__input:focus {
  outline: 1px solid var(--accent);
}

/* 站址是长字符串，给它更宽的一档；操作格本身允许换行，放不下时按钮会落到下一行 */
.ns__input--wide {
  width: 240px;
  max-width: 100%;
  flex: 1 1 200px;
}

/*
 * 原生下拉在深色主题下会白底（WebKitGTK 按系统主题画）—— 只能自己画：
 * `appearance: none` 去掉原生外观，再补一个箭头。
 */
select.ns__input {
  appearance: none;
  -webkit-appearance: none;
  padding-right: 26px;
  background-color: var(--field-bg);
  background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 12 12' fill='none' stroke='%23888' stroke-width='1.6' stroke-linecap='round'%3E%3Cpath d='M3 4.5 6 7.5 9 4.5'/%3E%3C/svg%3E");
  background-repeat: no-repeat;
  background-position: right 8px center;
  background-size: 11px;
  color: var(--text);
  cursor: pointer;
}

/* 展开的选项列表也跟随主题（否则仍是白底黑字） */
select.ns__input option {
  background-color: var(--field-bg);
  color: var(--text);
}

/* ---------- 按钮 ---------- */

.ns__btn {
  padding: 4px 11px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background-color: transparent;
  color: var(--text);
  font-size: 12.5px;
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

.ns__create .ns__btn--big {
  padding: 6px 14px;
  font-size: 13px;
}

.ns__notice {
  display: flex;
  align-items: baseline;
  gap: 8px;
  margin: 0 0 12px;
  padding: 8px 12px;
  border: 1px solid var(--border);
  border-left: 3px solid var(--text-dim);
  border-radius: 6px;
  background: var(--surface);
  color: var(--text);
  font-size: 12.5px;
  line-height: 1.6;
}

.ns__notice strong {
  flex-shrink: 0;
  font-weight: 500;
}

/*
 * 窄窗口改成一列。
 *
 * 断点按**容器**算，不是窗口：正文两侧各有 32px 内边距，媒体查询量的却是视口 ——
 * 所以阈值要留在 900px 左右，否则 800 多像素的窗口里表格已经溢出（"清空"被切掉），
 * 而这个媒体查询还没生效。
 */
@media (max-width: 900px) {
  .ns__head-row {
    display: none;
  }

  .ns__item {
    grid-template-columns: 1fr;
  }

  .ns__cell--actions {
    justify-content: flex-start;
  }
}
</style>
