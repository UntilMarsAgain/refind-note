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
import { BookOpen, Code } from "@lucide/vue";
import type { HelpPage } from "../../ipc/help.ts";
import { useFindInPage } from "../../composables/useFindInPage.ts";
import { useOutline } from "../../composables/useOutline.ts";
import FindBar from "../note/FindBar.vue";
import NoteContent from "../note/NoteContent.vue";
import OutlinePanel from "../note/OutlinePanel.vue";
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
  /**
   * 地址里带的章节（`#某节`）—— 目录上那一项要跟着它亮。
   *
   * 理由与 `NoteView` 里那个同名 prop 一样：直接打开 `Help:语法速览#模板块`
   * 时，目录什么都不指是另一回事。
   */
  section?: string;
  /** 上层让"打开查找条"（递增的信号，理由见 `NoteView` 里同样的字段） */
  findRequest?: number;
  /** 上层让"下一个/上一个" */
  findStep?: { step: number; dir: "next" | "previous" };
}>();

// 给默认值的理由：这两个是**可选** prop，不给默认值的话 watch 回调里拿到的是
// `undefined`，每次都得判一遍"有没有信号"—— 而"没给"与"给了 0"在这里本来就是
// 同一件事（0 = 还没触发过）。
const findRequest = computed(() => props.findRequest ?? 0);
const findStep = computed(() => props.findStep ?? { step: 0, dir: "next" as const });

const emit = defineEmits<{
  (e: "navigate", input: string): void;
  (e: "section", id: string): void;
  (e: "toggle-star"): void;
  /**
   * 当前页里有可查的正文。
   *
   * 带的是 `true`/`false`：全局那个 `find` 动作在 `App.vue`，
   * 而"这一页有没有正文可查"只有这一层知道。
   * 摊开源码那一支（`props.source`）报 false —— 那是编辑器，找是编辑器的事。
   */
  (e: "find", available: boolean): void;
}>();

const find = useFindInPage();

/** `NoteContent` 用 `defineExpose` 交出来的正文容器（查找要往里包 `<mark>`） */
const contentRef = ref<InstanceType<typeof NoteContent> | null>(null);

const entry = ref<HelpPage | null>(null);
const problem = ref("");
const loading = ref(false);

/**
 * 本页目录。
 *
 * 与阅读笔记走**同一个** composable（`useOutline`）—— 两页是同一件事，
 * 不该各写一遍"什么时候重数一次"。帮助页比笔记更需要它：《语法展示》那一页
 * 有二十几 KB、几十个标题，没有目录就只能一路滚。
 */
const outline = useOutline(contentRef);

// 下面这几个 watch 里有两个是 `immediate: true` —— 它们**在 setup 还没走完时就跑一遍**。
// 所以凡是它们用到的状态（`entry`）必须声明在上面。我第一版把这一整块插在
// 状态声明**之前**，于是首次打开帮助页必然抛
// `ReferenceError: Cannot access 'entry' before initialization`（TDZ）。
//
// 症状很轻（页面照常显示，只是控制台里两声报错），所以很容易被当成"无害噪音"
// 放过 —— 但它意味着那两个 `immediate` 的首轮回调其实**没跑成**，
// `findable` 报的仍是初值。改这块顺序时留意。

// 把容器交给查找层；换了容器就收掉高亮（它属于旧正文）
watch(
  contentRef,
  (component) => {
    find.root.value = component?.rootEl ?? null;
    find.hide();
  },
  { immediate: true },
);

// 地址里带来的章节（深链 / 后退回来）：目录上那一项也要亮
watch(
  () => props.section ?? "",
  (section) => outline.sync(section),
  { immediate: true },
);

// 告诉上层"这里能查/不能查"：正文那一支能查，源码那一支不归它
watch(
  [() => props.source, () => entry.value],
  () => emit("find", !props.source && entry.value !== null),
  { immediate: true },
);

// 上层让"打开查找条"（全局 `find` 动作走这条线）
watch(
  () => findRequest.value,
  (value) => {
    if (value > 0) {
      find.show();
    }
  },
);

// 上层让"下一个/上一个"
watch(
  () => findStep.value,
  (value, previous) => {
    if (value.step === previous.step || !find.open.value) {
      return;
    }
    if (value.dir === "next") {
      find.next();
    } else {
      find.previous();
    }
  },
);

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

/** 目录里点一条：滚过去，并把章节报给上层（与正文里点锚点是同一条路） */
function onPickSection(id: string) {
  outline.pick(id);
  emit("section", id);
}

/** 正文里点锚点：目录上那一项跟着亮 */
function onSection(id: string) {
  outline.mark(id);
  emit("section", id);
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

      <template v-else>
        <!--
          本页目录：只挂在**正文**那一支上 —— 摊开的源码是编辑器，
          那里有 CodeMirror 自己的查找与折叠，再摆一份目录只是重复。
        -->
        <OutlinePanel
            :entries="outline.entries.value"
            :active="outline.active.value"
            @pick="onPickSection"
        />

        <NoteContent
            ref="contentRef"
            :html="entry.html"
            @wikilink="emit('navigate', $event.title)"
            @wikilink-new="emit('navigate', $event)"
            @section="onSection"
        />
      </template>

      <!-- 页内查找：帮助页也是正文，一样能找（`find-bar.css` 写了层叠关系） -->
      <FindBar
          :open="find.open.value"
          :total="find.total.value"
          :index="find.index.value"
          :case-sensitive="find.caseSensitive.value"
          :whole-word="find.wholeWord.value"
          @query="find.search"
          @case-sensitive="find.caseSensitive.value = $event"
          @whole-word="find.wholeWord.value = $event"
          @refresh="find.refresh"
          @next="find.next"
          @previous="find.previous"
          @close="find.hide"
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
