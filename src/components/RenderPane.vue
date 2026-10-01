<script setup lang="ts">
import { computed, nextTick, ref, watch } from "vue";
import type { ResolvedAddress } from "../bindings/address.ts";
import { preferences } from "../preferences.ts";
import type { TabState } from "../tabs.ts";
import AllPages from "./AllPages.vue";
import DebugPage from "./DebugPage.vue";
import FilesPage from "./FilesPage.vue";
import GcPage from "./GcPage.vue";
import DeleteView from "./DeleteView.vue";
import HistoryView from "./HistoryView.vue";
import MissingView from "./MissingView.vue";
import NewTabView from "./NewTabView.vue";
import NoteEditor from "./NoteEditor.vue";
import NoteView from "./NoteView.vue";
import RollbackView from "./RollbackView.vue";
import SettingsPage from "./SettingsPage.vue";
import TrashPage from "./TrashPage.vue";
import UnlockView from "./UnlockView.vue";

/**
 * 渲染区。
 *
 * 只读 `tab.route`（**地址落到仓库上的结果**）—— 「在哪、是什么状态」的唯一真相。
 * 不解析地址（那是 `address.ts` 的活），不碰全局导航（后退 / 前进是标签页的历史），
 * 也不看地址栏里正在敲的那串字。视图要换，换的是从 `route` 派生出来的那一层。
 */
const props = defineProps<{
    tab: TabState | null;
}>();

// 正文里的内部链接要换地址，而地址归上层管 —— 渲染区只把意图交出去
const emit = defineEmits<{
    (e: "navigate", input: string): void;
    /** 上锁的页面改去 `@unlock`：这是**替换**当前这条记录，不是压新的一条 */
    (e: "redirect", input: string): void;
    (e: "navigate-new-tab", input: string): void;
    /** 点了正文里的锚点：把章节叠进地址（上层拼输入） */
    (e: "section", section: string): void;
    /** 编辑器提交完成或取消：它也要换地址，同样交给上层 */
    (e: "edit-navigate", input: string): void;
    /** 要看历史里的某一版。地址由上层拼 —— 渲染区不自己拼地址 */
    (e: "open-version", payload: { title: string; rev: number }): void;
    /** 要回退到历史里的某一版（同样是让上层拼地址） */
    (e: "rollback-version", payload: { title: string; rev: number }): void;
    /** 删掉了：上层该换个地方待着 */
    (e: "deleted"): void;
    /** 请求建立某一篇笔记（"还不存在"那一页上的按钮） */
    (e: "create-note", title: string): void;
    /** 回滚完成：带回新版本号 */
    (e: "rolled-back", rev: number): void;
    /** 某一页解开了锁：上层把地址换回去 */
    (e: "unlocked"): void;
    /** 从解锁页退出来：它换掉的那条记录已经不在历史里了，交给上层决定去哪 */
    (e: "unlock-cancel"): void;
}>();

/** 权威副本：渲染区的一切都从这里派生 */
const route = computed<ResolvedAddress | null>(() => props.tab?.route ?? null);

/** 收窄后的各支（模板里的 v-else-if 不会替我们收窄联合类型） */
const outcome = computed(() => route.value?.outcome ?? null);
const noteTitle = computed(() => (outcome.value?.kind === "note" ? outcome.value.title : null));
const missingTitle = computed(() =>
    outcome.value?.kind === "missing" ? outcome.value.title : null,
);
const specialPage = computed(() =>
    outcome.value?.kind === "special" ? outcome.value.page : null,
);

/** 「什么状态」：浏览状态在语法层，落到仓库上之后照样有效 */
const mode = computed(() => route.value?.address.mode ?? null);

/** 地址里带的章节。它当锚点用：`special:settings#accent` 跳到那一项 */
const section = computed(() => route.value?.address.section ?? "");

/**
 * 新标签页。
 *
 * 空地址也算它：刚打开程序、按下 Ctrl+T 都是一片空白，这时候该给的是**输入框**，
 * 而不是一句"这里是空的"。
 */
const isNewTab = computed(() => route.value === null || specialPage.value === "newtab");

/** 宽度限制器：偏好里的一项 */
const limited = computed(() => preferences.value.limit_width);

/** 滚动容器。滚动位置也属于"这个标签页的浏览状态"，所以存进标签页自己 */
const scroller = ref<HTMLElement | null>(null);

/**
 * 页头收起没有：正文滚过一点就收成一条细栏。
 *
 * 两个阈值（收起 32 / 展开 12）是**迟滞**：收起后页头变矮、正文跟着上移，
 * 用一个阈值就会在边界上抖个不停。
 */
const collapsed = ref(false);

function syncCollapsed(top: number) {
    collapsed.value = collapsed.value ? top > 12 : top > 32;
}

// 切标签页：还原它上次停下的位置。状态在标签页里，这里只是"读出来用"。
watch(
    () => props.tab?.id,
    async () => {
        await nextTick();
        const top = props.tab?.scroll ?? 0;
        scroller.value?.scrollTo(0, top);
        collapsed.value = top > 32;
    },
    { immediate: true },
);

function onScroll() {
    if (props.tab && scroller.value) {
        props.tab.scroll = scroller.value.scrollTop;
    }
    syncCollapsed(scroller.value?.scrollTop ?? 0);
}

/** 右下角那组按钮要能滚动渲染区，而滚动容器在这里 */
defineExpose({
    scrollToTop() {
        scroller.value?.scrollTo({ top: 0, behavior: "smooth" });
    },
    scrollToBottom() {
        scroller.value?.scrollTo({ top: scroller.value.scrollHeight, behavior: "smooth" });
    },
});
</script>

<template>
  <main ref="scroller" class="pane" :class="{ 'pane--wide': !limited }" @scroll.passive="onScroll">
    <div class="pane__column" :class="{ 'pane__column--wide': !limited }">
      <!-- 理论上窗口里至少有一个标签页，这条是兜底 -->
      <p v-if="!tab" class="pane__hint">没有标签页。</p>

      <SettingsPage v-else-if="specialPage === 'settings'" :focus="section"/>

      <DebugPage v-else-if="specialPage === 'debug'"/>

      <FilesPage v-else-if="specialPage === 'files'"/>

      <TrashPage
          v-else-if="specialPage === 'trash'"
          @navigate="emit('navigate', $event)"
      />

      <GcPage v-else-if="specialPage === 'gc'" @navigate="emit('navigate', $event)"/>

      <AllPages v-else-if="specialPage === 'all'" @navigate="emit('navigate', $event)"/>

      <NewTabView v-else-if="isNewTab" @open="emit('navigate', $event)"/>

      <template v-else-if="noteTitle && mode">
        <NoteView
            v-if="mode.kind === 'view'"
            :key="`${noteTitle}@${mode.ref ?? ''}`"
            :title="noteTitle"
            :reference="mode.ref"
            :collapsed="collapsed"
            @navigate="emit('navigate', $event)"
            @redirect="emit('redirect', $event)"
            @navigate-new-tab="emit('navigate-new-tab', $event)"
            @section="emit('section', $event)"
        />

        <NoteEditor
            v-else-if="mode.kind === 'edit'"
            :key="noteTitle"
            :title="noteTitle"
            @navigate="emit('edit-navigate', $event)"
        />

        <HistoryView
            v-else-if="mode.kind === 'history'"
            :title="noteTitle"
            @open-version="emit('open-version', { title: noteTitle, rev: $event })"
            @rollback="emit('rollback-version', { title: noteTitle, rev: $event })"
            @cancel="emit('navigate', noteTitle)"
        />

        <DeleteView
            v-else-if="mode.kind === 'delete'"
            :title="noteTitle"
            @cancel="emit('navigate', noteTitle)"
            @deleted="emit('deleted')"
        />

        <RollbackView
            v-else-if="mode.kind === 'rollback'"
            :title="noteTitle"
            :reference="mode.ref"
            @cancel="emit('navigate', `${noteTitle}@view-${mode.ref}`)"
            @rolled-back="emit('rolled-back', $event)"
        />

        <UnlockView
            v-else-if="mode.kind === 'unlock'"
            :title="noteTitle"
            :reference="mode.ref"
            @navigate="emit('navigate', $event)"
            @cancel="emit('unlock-cancel')"
            @unlocked="emit('unlocked')"
        />
      </template>

      <MissingView
          v-else-if="missingTitle"
          :title="missingTitle"
          @create="emit('create-note', missingTitle)"
      />

      <!-- 兜底：到不了这里，真到了也别白屏 -->
      <p v-else class="pane__hint">这一页还没有对应的视图。</p>
    </div>
  </main>
</template>

<style scoped>
.pane {
  flex: 1 1 auto;
  /* 允许被标签栏挤窄，否则里面的长地址会把整行撑开 */
  min-width: 0;
  overflow: auto;
  padding: 0 32px 64px;
  /* 开合宽度限制器时，右边这块留白也一起变 */
  transition: padding 200ms ease;
}

/*
 * 关掉限宽后正文一路铺到右边，而右下角那组按钮正压在那里 ——
 * 让出它的宽度，免得字被盖住。
 */
.pane--wide {
  padding-right: 64px;
}

/* 阅读栏：页头与正文共用同一条，限宽并居中 */
.pane__column {
  max-width: var(--reading-width);
  /* 居中不能省：关掉限宽时这一列会一路长到铺满，中间那一段正是靠它保持对称的 */
  margin-inline: auto;
  /* 开合宽度限制器时平滑变化，而不是硬跳 */
  transition: max-width 200ms ease;
}

/*
 * 关掉宽度限制器：铺满窗口。
 *
 * 这里必须用 `100vw`（一个**长度**），不能用 `100%`：`max-width` 从 px 到百分比
 * 插值不可靠，过渡会直接断掉、看着就是"没有动画"。两个都是长度才插得动。
 * 它比可用宽度大，所以这一列自然铺满；`margin-inline: auto` 这时算出来是 0，
 * 保持 auto 反而让中间每一帧都是居中的。
 */
.pane__column--wide {
  max-width: 100vw;
}

@media (prefers-reduced-motion: reduce) {
  .pane,
  .pane__column {
    transition: none;
  }
}

.pane__hint {
  margin: 28px 0 0;
  color: var(--text-dim);
  font-size: 13.5px;
}
</style>
