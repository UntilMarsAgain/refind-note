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
import { Plus } from "@lucide/vue";
import {
  anchorOf,
  clearable,
  kindOf,
  reserved,
  titleOf,
  useNamespaceTable,
} from "../../composables/useNamespaceTable.ts";

/**
 * 命名空间管理（设置页的一节，地址 `special:settings#namespaces`）。
 *
 * 这里只摆版面：表尾的新建表单、每行的三处行内编辑、两步确认的清空 / 删除。
 * 状态与动作全在 `composables/useNamespaceTable.ts` 里 —— 连同"哪些行不许动、
 * 为什么"那几个纯函数（`reserved` / `clearable` / `kindOf`）。
 *
 * 命令一律返回**更新后的整张表**，所以这里不需要"改完再拉一次"；校验的权威在后端，
 * 这里只挡明显的空输入（`canCreate`）。
 */
const {
  items,
  busy,
  error,
  loading,
  editing,
  aliasDraft,
  aliasInput,
  siteDraft,
  nameDraft,
  armed,
  newName,
  newSite,
  newKind,
  canCreate,
  startAliases,
  addAlias,
  dropAlias,
  saveAliases,
  startSite,
  saveSite,
  startRename,
  saveRename,
  emptyNamespace,
  deleteNamespace,
  create,
} = useNamespaceTable();
</script>

<template>
  <section class="ns">
    <p class="ns__lead">
      命名空间是标题的前缀（如 `帮助:入门`）。<strong>标识与名称分开保存</strong>，
      因此改名不会移动任何文件。配置了站点地址的命名空间用于跨站链接
      （`[[zhwiki:条目]]` 以绿色显示，交由浏览器打开），其页面不在本仓库中。
    </p>

    <p v-if="error" class="ns__notice">{{ error }}</p>
    <p v-if="loading" class="ns__hint">正在读…</p>

    <div v-else class="ns__table">
      <div v-for="item in items" :key="item.id" :id="anchorOf(item)" class="ns__row">
        <div class="ns__head">
          <span class="ns__name">{{ titleOf(item) }}</span>
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
            v-if="clearable(item)"
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
            其中页面将移入回收站（可还原）
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
      名称与别名不得重复（忽略大小写与空白），且不能包含 `/` `:` `@` `#` 等字符。
      别名仅表示"也接受这种写法"，显示时始终使用规范名称。
    </p>
  </section>
</template>

<style scoped src="./namespace.css"/>
