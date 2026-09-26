<script setup lang="ts">
/**
 * 数据库回收（`special:gc`）。
 *
 * 只做一件事：让用户手动跑一次回收，并把结果如实报出来。
 * 两个选项各自对应后端 `gc(orphan_blobs, superseded_drafts)` 的一个开关 ——
 * 界面不改语义，只是把那个命令摆到台面上（删除确认页里也用它）。
 */
import { ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import ViaHint from "./ViaHint.vue";
const props = defineProps<{
  /** 跟重定向来到这一页时的来源提示（空串 = 直接打开） */
  via?: string;
}>();

const orphanBlobs = ref(true);
const supersededDrafts = ref(true);
/** 先清空回收站：那些笔记的内容块这时才成为孤块，可以一并回收 */
const purgeTrashFirst = ref(false);
/** 已提交的任务 id（提交本身是瞬时的，所以这里只用来给一句反馈） */
const submitted = ref<number | null>(null);
const error = ref("");

/**
 * 只**提交任务**，不等它跑完：回收是长期操作，进度与结果都在底部任务栏。
 * 这样按钮不会长时间处于"忙"状态，也可以连续提交。
 */
async function run() {
  error.value = "";
  submitted.value = null;
  try {
    submitted.value = await invoke<number>("submit_gc", {
      orphanBlobs: orphanBlobs.value,
      supersededDrafts: supersededDrafts.value,
      purgeTrashFirst: purgeTrashFirst.value,
    });
  } catch (reason) {
    error.value = String(reason);
  }
}
</script>

<template>
  <section class="gc">
    <h1 class="gc__title">数据库回收</h1>
    <ViaHint :hint="via ?? ''" />
    <p class="gc__lead">
      回收两类不再被引用的数据。<strong>历史版本引用的内容不会被回收</strong>，
      包括已删除笔记（在 <code>trash/</code> 里）所引用的。
    </p>

    <ul class="gc__options">
      <li>
        <label class="gc__option">
          <input v-model="orphanBlobs" type="checkbox" />
          <span class="gc__text">
            <strong>孤立数据块</strong>
            <em>没有任何版本引用的内容块。</em>
          </span>
        </label>
      </li>
      <li>
        <label class="gc__option">
          <input v-model="supersededDrafts" type="checkbox" />
          <span class="gc__text">
            <strong>已被取代的草稿</strong>
            <em>提交时被取代、之后不会再被读到的草稿节点。</em>
          </span>
        </label>
      </li>
      <li>
        <label class="gc__option">
          <input v-model="purgeTrashFirst" type="checkbox" />
          <span class="gc__text">
            <strong>先清空回收站</strong>
            <em>
              回收站里的笔记先全部删掉，它们的内容块这时才成为孤块，可以顺带回收。
              这一步<strong>不可撤销</strong>（回收站里那些笔记将不再能还原）。
            </em>
          </span>
        </label>
      </li>
    </ul>

    <div class="gc__actions">
      <button
        class="gc__run"
        type="button"
        :disabled="!orphanBlobs && !supersededDrafts"
        @click="run"
      >
        开始回收
      </button>
      <span class="gc__warn">回收会删除这些数据，无法撤销。</span>
    </div>

    <p v-if="error" class="gc__error">提交失败：{{ error }}</p>
    <p v-else-if="submitted !== null" class="gc__submitted">
      已提交（任务 #{{ submitted }}），进度与结果见左下角任务栏。
    </p>
  </section>
</template>

<style scoped>
.gc {
  /* 宽度交给 App 的阅读栏（`.app__column`）：限宽时 1080，不限宽时铺满。
     这里自设 max-width 会把它盖住，页面对"限宽"按钮就没反应了。 */
  margin: 0 auto;
  padding: 28px 20px 64px;
}

.gc__title {
  margin: 0 0 6px;
  font-size: 22px;
}

.gc__lead {
  margin: 0 0 20px;
  color: var(--text-dim);
  font-size: 13px;
  line-height: 1.7;
}

.gc__lead code {
  padding: 1px 5px;
  border-radius: 4px;
  background: var(--code-bg);
  font-size: 12px;
}

.gc__options {
  margin: 0 0 18px;
  padding: 0;
  list-style: none;
}

.gc__option {
  display: flex;
  align-items: flex-start;
  gap: 10px;
  padding: 8px 0;
  cursor: pointer;
}

.gc__text {
  display: flex;
  flex-direction: column;
  gap: 2px;
  font-size: 14px;
}

.gc__text em {
  color: var(--text-dim);
  font-size: 12px;
  font-style: normal;
}

.gc__actions {
  display: flex;
  align-items: center;
  gap: 12px;
  flex-wrap: wrap;
}

.gc__run {
  padding: 6px 14px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background-color: transparent;
  color: var(--text);
  font-size: 13px;
  cursor: pointer;
}

.gc__run:hover:not(:disabled) {
  background-color: var(--hover);
}

.gc__run:disabled {
  opacity: 0.45;
  cursor: default;
}

.gc__warn {
  color: var(--text-dim);
  font-size: 12px;
}

.gc__submitted {
  margin-top: 14px;
  color: var(--text-dim);
  font-size: 13px;
}

.gc__error {
  margin-top: 16px;
  color: var(--link-missing);
  font-size: 13px;
}

</style>
