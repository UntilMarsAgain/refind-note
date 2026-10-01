<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { BookOpen, Code } from "@lucide/vue";
import type { HelpPage } from "../../bindings/help.ts";
import NoteContent from "../note/NoteContent.vue";
import PageHeader, { type PageAction } from "../note/PageHeader.vue";
import SourceView from "../note/SourceView.vue";

/**
 * 帮助页（`Help:入门`）。
 *
 * 内容**随程序发布**，不在仓库里，所以这一页是只读的：`@edit` 只把源码摊开给人看
 * （用编辑器同一套 CodeMirror，但**不许编辑**），不改任何东西 ——
 * 改帮助要去改仓库里 `help/` 下的文件，再重新编译。
 *
 * 渲染与笔记走的是同一个渲染器（后端一处），所以帮助里的模板块、代码块、表格
 * 与笔记里长得一模一样。
 */
const props = defineProps<{
  /** 页面名（地址里那一段） */
  page: string;
  /** 显示标题（`Help:入门`），由后端解析给出 */
  display: string;
  /** 地址状态是 `@edit`：摊开源码看 */
  source: boolean;
  /** 正文滚下去了：页头收起 */
  collapsed: boolean;
  /** 这一页能不能改 —— 由后端给（帮助页给的是"不能"） */
  editable: boolean;
  /** 这一页星标过没有 */
  starred?: boolean;
}>();

const emit = defineEmits<{
  (e: "navigate", input: string): void;
  (e: "section", id: string): void;
  (e: "toggle-star"): void;
}>();

const entry = ref<HelpPage | null>(null);
const problem = ref("");
const loading = ref(false);

async function load() {
  loading.value = true;
  problem.value = "";
  try {
    entry.value = await invoke<HelpPage>("read_help", { page: props.page });
  } catch (reason) {
    entry.value = null;
    problem.value = String(reason);
  } finally {
    loading.value = false;
  }
}

watch(() => props.page, () => void load(), { immediate: true });

/**
 * 页头上的动作：在"看"与"看源码"之间来回。
 *
 * 能不能改由后端给（`editable`）—— 改得了的页面本就走阅读视图那条路，
 * 走到这里的是**改不了**的那些（帮助页）：所以这里只切"看"与"看源码"。
 */
const actions = computed<PageAction[]>(() => [
  props.source
    ? { name: "read", label: "返回阅读", icon: BookOpen }
    : { name: "source", label: "查看源码", icon: Code },
]);

function onAction() {
  const slug = entry.value?.slug ?? props.page;
  emit("navigate", props.source ? `Help:${slug}` : `Help:${slug}@edit`);
}
</script>

<template>
  <div class="help">
    <p v-if="loading" class="help__hint">正在读取…</p>

    <div v-else-if="problem" class="help__error">
      <p class="help__error-text">{{ problem }}</p>
      <button type="button" class="help__btn" @click="load">重试</button>
    </div>

    <template v-else-if="entry">
      <PageHeader
          :title="display"
          parent=""
          :collapsed="props.collapsed"
          :actions="actions"
          :starred="props.starred ?? false"
          @action="onAction"
          @toggle-star="emit('toggle-star')"
      />

      <p v-if="!props.editable" class="help__note">
        帮助内容随程序发布，无法在这里编辑。
      </p>

      <!-- 源码：只读摊开（编辑器同一套视图）。改它要去改仓库里的帮助文件，再重新编译 -->
      <SourceView v-if="props.source" :markdown="entry.markdown"/>

      <NoteContent
          v-else
          :html="entry.html"
          @wikilink="emit('navigate', $event.title)"
          @wikilink-new="emit('navigate', $event)"
          @section="emit('section', $event)"
      />
    </template>
  </div>
</template>

<style scoped>
.help__note {
  margin: 10px 0 0;
  color: var(--text-dim);
  font-size: 12.5px;
}

.help__hint {
  margin: 28px 0 0;
  color: var(--text-dim);
  font-size: 13.5px;
}

.help__error {
  margin: 28px 0 0;
  padding: 12px 14px;
  border: 1px solid var(--danger);
  border-left-width: 3px;
  border-radius: 8px;
  color: var(--text);
  font-size: 13.5px;
}

.help__error-text {
  margin: 0 0 10px;
}

.help__btn {
  padding: 5px 12px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: transparent;
  color: var(--text-dim);
  font: inherit;
  font-size: 12.5px;
  cursor: pointer;
}

.help__btn:hover {
  border-color: var(--accent-soft);
  background: var(--accent-tint);
  color: var(--text);
}
</style>
