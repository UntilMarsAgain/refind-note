<script setup lang="ts">
import { ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { ClockArrowDown, Pencil, RotateCcw, Trash } from "@lucide/vue";
import type { Note, Reading } from "../bindings/note.ts";
import NoteContent from "./NoteContent.vue";
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
}>();

const emit = defineEmits<{
    /** 点了内部链接：算一次跳转 */
    (e: "navigate", title: string): void;
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
                "navigate",
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
      <p v-if="older()" class="note__older">
        这是第 {{ note.rev }} 版，不是最新版。
        <button type="button" class="note__link" @click="emit('navigate', note.title)">
          回到最新版
        </button>
      </p>

      <header class="note__head">
        <div class="note__line">
          <h1 class="note__title">{{ note.title }}</h1>

          <div class="note__actions">
            <button
                v-if="props.reference === null"
                type="button"
                class="note__action"
                title="编辑这一篇"
                @click="emit('navigate', `${note.title}@edit`)"
            >
              <Pencil :size="14" :stroke-width="1.9"/>
              编辑
            </button>
            <button
                type="button"
                class="note__action"
                title="看这一篇的所有版本"
                @click="emit('navigate', `${note.title}@history`)"
            >
              <ClockArrowDown :size="14" :stroke-width="1.9"/>
              历史
            </button>
            <button
                v-if="props.reference === null"
                type="button"
                class="note__action note__action--danger"
                title="删除这一篇（日志会挪进回收站，还能捞回来）"
                @click="emit('navigate', `${note.title}@delete`)"
            >
              <Trash :size="14" :stroke-width="1.9"/>
              删除
            </button>
            <button
                v-if="props.reference !== null"
                type="button"
                class="note__action"
                title="把这一版的内容作为新的一版写上去"
                @click="emit('navigate', `${note.title}@rollback-${props.reference}`)"
            >
              <RotateCcw :size="14" :stroke-width="1.9"/>
              回退到这一版
            </button>
          </div>
        </div>

        <p class="note__meta">
          <span>第 {{ note.rev }} 版</span>
          <span>改于 {{ note.modified }}</span>
          <StorageBadge :protection="note.protection"/>
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
  margin: 20px 0 0;
  padding: 8px 12px;
  border-left: 3px solid var(--accent-soft);
  border-radius: 6px;
  background: var(--accent-tint);
  color: var(--text-dim);
  font-size: 13px;
}

.note__link {
  padding: 0;
  border: 0;
  background: none;
  color: var(--accent-soft);
  font: inherit;
  text-decoration: underline;
  cursor: pointer;
}

.note__head {
  padding: 28px 0 8px;
}

/* 标题占满剩下的宽度，动作按钮靠右 —— 长标题换行时按钮不会被挤走 */
.note__line {
  display: flex;
  gap: 12px;
  align-items: baseline;
  justify-content: space-between;
}

.note__actions {
  display: flex;
  flex: none;
  gap: 6px;
}

.note__action {
  display: inline-flex;
  gap: 5px;
  align-items: center;
  padding: 5px 11px;
  border: 1px solid var(--border);
  border-radius: 999px;
  background: transparent;
  color: var(--text-dim);
  font: inherit;
  font-size: 12.5px;
  line-height: 1.3;
  white-space: nowrap;
  cursor: pointer;
  transition:
      background-color 120ms ease,
      color 120ms ease,
      border-color 120ms ease;
}

.note__action:hover {
  border-color: var(--accent-soft);
  background: var(--accent-tint);
  color: var(--text);
}

/* 只有"删除"用危险色，免得一串按钮里看不出哪个是破坏性的 */
.note__action--danger:hover {
  border-color: var(--danger);
  background: transparent;
  color: var(--danger);
}

.note__title {
  margin: 0;
  font-size: 26px;
  font-weight: 600;
  line-height: 1.35;
  overflow-wrap: anywhere;
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
