<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { ArrowLeft, History, RotateCcw } from "@lucide/vue";
import type { Note, Reading } from "../../bindings/note.ts";
import { parentOf } from "../../core/title.ts";
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

    <p v-else-if="error" class="note__error">{{ error }}</p>

    <template v-else-if="note">
      <PageHeader
          :title="note.title"
          :parent="parentOf(note.title)"
          :collapsed="props.collapsed"
          :actions="headerActions"
          @action="onAction"
          @open-parent="emit('navigate', $event)"
      />

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
</style>
