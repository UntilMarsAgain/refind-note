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
import { computed, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { ArrowLeft, Download, History, RotateCcw } from "@lucide/vue";
import type { Via } from "../../ipc/address.ts";
import type { Note, Reading } from "../../ipc/note.ts";
import { parentOf } from "../../core/title.ts";
import { flash } from "../../core/notice.ts";
import { saveNoteMarkdown } from "../../dom/file-save.ts";
import NoteContent from "./NoteContent.vue";
import PageHeader, { type PageAction } from "./PageHeader.vue";
import StorageBadge from "./StorageBadge.vue";

/**
 * 一篇笔记的阅读视图。
 *
 * 拉数据在这里，正文渲染交给 `NoteContent` —— 后者只吃一个 HTML 字符串，
 * 不认识"笔记"这个概念。
 *
 * 页头顺带把**这一版在磁盘上是怎么存的**挂出来：值来自 blob 的明文头，
 * 所以不解锁也显示得出来。
 *
 * 给了 `reference` 就读 token 指的那一版（`@view-<token>`）。它是只读的：
 * 看旧版本时不提供编辑，免得把草稿写到一个已经在看历史的状态上。
 */
const props = defineProps<{
    title: string;
    /** 读哪一版（地址里的版本 token）；null 就是最新一版 */
    reference: string | null;
    /** 正文滚下去了：页头收起成一条细栏 */
    collapsed: boolean;
    /** 被指令带过来时的"从哪儿来" */
    via?: Via | null;
    /** 这一页星标过没有 */
    starred?: boolean;
}>();

const emit = defineEmits<{
    /** 点了内部链接：算一次跳转 */
    (e: "navigate", title: string): void;
    /**
     * 上锁时改去 `@unlock`：走**替换**而不是压新记录 ——
     * 否则从解锁页后退会回到这篇、又被重定向回来，来回打转。
     */
    (e: "redirect", input: string): void;
    /** Ctrl/Cmd+点击内部链接：在新标签页打开 */
    (e: "navigate-new-tab", title: string): void;
    /** 点了页内锚点：章节交给上层叠进地址 */
    (e: "section", id: string): void;
    /**
     * 读不出来时退一步（退回上一页）。
     *
     * 出错的原因可能是"口令没输"这种本可以避免的事，把人困在一条报错上没有道理 ——
     * 页面要留一条走得通的路。
     */
    (e: "leave"): void;
    /** 加/去星标 */
    (e: "toggle-star"): void;
}>();

const note = ref<Note | null>(null);
const error = ref("");
const loading = ref(false);

/** 看的是不是一个旧版本（用来决定要不要提示"这不是最新版"） */
const older = () => props.reference !== null && Number(props.reference) !== note.value?.rev;

/**
 * 页头的动作。
 *
 * 看最新版时用页头自带那三个（编辑 / 版本历史 / 删除）；看旧版本时是**另一套** ——
 * 那一版是只读的，能做的只有"回去""把这一版重新提上去""看看历史"。
 */
const headerActions = computed<PageAction[] | undefined>(() => {
    const found = note.value;
    if (!found || props.reference === null) {
        return undefined;
    }
    return [
        { name: "back", label: `返回「${found.title}」`, icon: ArrowLeft },
        { name: "rollback", label: "回退到这一版", icon: RotateCcw },
        { name: "history", label: "版本历史", icon: History },
        // 旧版也能导出：要的是那一版的原文，与"能不能编辑"无关
        { name: "export", label: "导出", icon: Download },
    ];
});

/** 页头按下的动作名 → 地址（拼输入；规范地址仍由后端解析出来） */
function onAction(name: string) {
    const found = note.value;
    if (!found) {
        return;
    }
    switch (name) {
        case "edit":
            emit("navigate", `${found.title}@edit`);
            break;
        case "history":
            emit("navigate", `${found.title}@history`);
            break;
        case "delete":
            emit("navigate", `${found.title}@delete`);
            break;
        case "back":
            emit("navigate", found.title);
            break;
        case "rollback":
            emit("navigate", `${found.title}@rollback-${props.reference}`);
            break;
        case "export":
            void exportMarkdown();
            break;
    }
}

/**
 * 导出这一版的 markdown 原文。
 *
 * 内容由后端从仓库里读（前端不转手），路径由系统保存对话框给出 ——
 * 与"另存为一份文件"是同一条路。
 */
async function exportMarkdown() {
    try {
        const target = await saveNoteMarkdown(props.title, props.reference);
        if (target) {
            flash(`已导出：${target}`);
        }
    } catch (error) {
        flash(`导出失败：${error}`);
    }
}

async function load() {
    loading.value = true;
    error.value = "";

    try {
        const reading = await invoke<Reading>("read_note", {
            title: props.title,
            reference: props.reference,
        });

        // 上了锁就把地址换成 `@unlock`：输入口令的地方是**那一页**，不是这里。
        // 看的是旧版本就带上版本号 —— 口令按版本存，要解的是那一版。
        if (reading.state === "locked") {
            note.value = null;
            emit(
                "redirect",
                props.reference === null
                    ? `${props.title}@unlock`
                    : `${props.title}@unlock-${props.reference}`,
            );
            return;
        }

        note.value = reading.note;
    } catch (reason) {
        note.value = null;
        error.value = String(reason);
    } finally {
        loading.value = false;
    }
}

// 换一篇或换一版就重读。标题是这一页的身份，地址里其余成分（状态、章节）由上层管。
watch(
    () => [props.title, props.reference],
    () => void load(),
    { immediate: true },
);
</script>

<template>
  <div class="note">
    <p v-if="loading" class="note__hint">正在读「{{ title }}」…</p>

    <div v-else-if="error" class="note__error">
      <p class="note__error-text">{{ error }}</p>
      <div class="note__error-actions">
        <button type="button" class="note__error-btn" @click="load">重试</button>
        <button type="button" class="note__error-btn" @click="emit('leave')">返回上一页</button>
      </div>
    </div>

    <template v-else-if="note">
      <PageHeader
          :title="note.title"
          :parent="parentOf(note.title)"
          :collapsed="props.collapsed"
          :actions="headerActions"
          :via="props.via ?? null"
          :starred="props.starred ?? false"
          @action="onAction"
          @open-parent="emit('navigate', $event)"
          @open-via="emit('navigate', $event)"
          @toggle-star="emit('toggle-star')"
      />

      <!--
        指令页面（正文第一行是 `$$COMMAND$$`）：它是**程序做的事**，不是给人读的正文。
        所以这里说清它会干什么，并给一条"照它跳一次"的路 —— 跟跳走的是地址，
        与打开这一页时发生的事完全一样。
      -->
      <div v-if="note.command" class="command" :class="{ 'command--bad': note.command.kind === 'unrecognized' }">
        <span class="command__label">{{ note.command.label }}</span>
        <span class="command__detail">{{ note.command.detail }}</span>
        <button
            v-if="note.command.argument"
            type="button"
            class="command__go"
            title="按这条指令跳过去"
            @click="emit('navigate', note.command.argument)"
        >
          {{ note.command.argument }}
        </button>
      </div>

      <p v-if="older()" class="note__older">
        这是第 {{ note.rev }} 版，不是最新版。
      </p>

      <header class="note__head">
        <p class="note__meta">
          <span>第 {{ note.rev }} 版</span>
          <span>改于 {{ note.modified }}</span>
          <StorageBadge :protection="note.protection" :title="note.title" :reference="props.reference"/>
        </p>
        <p v-if="note.summary" class="note__summary">{{ note.summary }}</p>
      </header>

      <NoteContent
          :html="note.html"
          @wikilink="emit('navigate', $event.title)"
          @wikilink-new="emit('navigate-new-tab', $event)"
          @section="emit('section', $event)"
      />
    </template>
  </div>
</template>

<style scoped>
/* 看旧版本时的提示条。它不是错误，所以不报警告色，只让人知道自己在看历史 */
.note__older {
  display: flex;
  flex-wrap: wrap;
  gap: 0 6px;
  align-items: center;
  margin: 12px 0 0;
  padding: 8px 12px;
  border-left: 3px solid var(--accent-soft);
  border-radius: 6px;
  background: var(--accent-tint);
  color: var(--text-dim);
  font-size: 13px;
}

/* 页头（标题与动作）现在归 PageHeader；这里只剩标题下面那几行事实 */
.note__head {
  padding: 4px 0 8px;
}

/* 指令页的提示条：这一页是给程序的，不是给人读的 */
.command {
  display: flex;
  flex-wrap: wrap;
  gap: 6px 10px;
  align-items: baseline;
  margin: 12px 0 0;
  padding: 8px 12px;
  border-left: 3px solid var(--accent-soft);
  border-radius: 6px;
  background: var(--accent-tint);
  color: var(--text-dim);
  font-size: 13px;
}

/* 指令写坏了：那是错误，不是提示 */
.command--bad {
  border-left-color: var(--danger);
  background: transparent;
  border-top: 1px solid var(--danger);
  border-right: 1px solid var(--danger);
  border-bottom: 1px solid var(--danger);
}

.command__label {
  color: var(--text);
  font-weight: 600;
}

.command__detail {
  flex: 1 1 auto;
  min-width: 0;
}

.command__go {
  padding: 2px 10px;
  border: 1px solid var(--border);
  border-radius: 999px;
  background: transparent;
  color: var(--accent-soft);
  font: inherit;
  font-size: 12.5px;
  cursor: pointer;
}

.command__go:hover {
  border-color: var(--accent-soft);
  background: var(--accent-soft);
  color: var(--bg);
}

.note__meta {
  display: flex;
  flex-wrap: wrap;
  gap: 6px 14px;
  align-items: center;
  margin: 6px 0 0;
  color: var(--text-dim);
  font-size: 13px;
}

.note__summary {
  margin: 8px 0 0;
  color: var(--text-dim);
  font-size: 13px;
}

.note__hint {
  margin: 28px 0 0;
  color: var(--text-dim);
  font-size: 13.5px;
}

.note__error {
  margin: 28px 0 0;
  padding: 12px 14px;
  border: 1px solid var(--danger);
  border-left-width: 3px;
  border-radius: 8px;
  background: var(--surface);
  color: var(--text);
  font-size: 13.5px;
}

.note__error-text {
  margin: 0;
  line-height: 1.7;
}

/* 报错也要有出路：重试，或者退回去看别的 */
.note__error-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin-top: 10px;
}

.note__error-btn {
  padding: 5px 12px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: transparent;
  color: var(--text-dim);
  font: inherit;
  font-size: 12.5px;
  cursor: pointer;
}

.note__error-btn:hover {
  border-color: var(--accent-soft);
  background: var(--accent-tint);
  color: var(--text);
}
</style>
