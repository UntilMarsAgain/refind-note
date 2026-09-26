<script setup lang="ts">
import type { Task } from "../bindings";
/**
 * 底部任务状态栏。
 *
 * 长期操作（数据库整理，以后的网络同步）只提交任务，进度与结果在这里显示 ——
 * 用户不必盯着某个按钮，也能知道后台在干什么、干完了没有。
 *
 * 轮询而不是事件：`list_tasks` 只是克隆一张小表，800ms 一次的开销可以忽略，
 * 换来的是"提交方不需要知道谁在看"（回收站、设置页、将来的同步都能提交）。
 */
import { computed, onBeforeUnmount, onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";


const emit = defineEmits<{ (e: "finished"): void }>();

const tasks = ref<Task[]>([]);
/** 上一轮各任务的状态：据此判断"刚刚结束" */
let lastStates: Record<number, Task["state"]> = {};
/** 用户已经关掉的那些（按 id 记，轮询回来的表里仍会有它们） */
const hidden = ref<number[]>([]);
let timer: number | undefined;

const active = computed(() =>
  tasks.value.filter((task) => task.state === "queued" || task.state === "running"),
);

/** 已经结束、用户还没关掉的，取最近一条 */
const lastFinished = computed(() => {
  const finished = tasks.value.filter(
    (task) => task.state === "done" || task.state === "failed",
  );
  const task = finished[finished.length - 1];
  return task && !hidden.value.includes(task.id) ? task : null;
});

/** 有活儿在跑就显示进度，否则显示最近一条结果 */
const runningTask = computed(() => active.value[active.value.length - 1] ?? null);
const show = computed(() => runningTask.value !== null || lastFinished.value !== null);

async function poll() {
  try {
    const next = await invoke<Task[]>("list_tasks");

    // 有任务从"排队/运行"变成"完成/失败"就通知上层：设置页里那两个
    // "上次执行时间"要重新读，否则界面会停在旧值（显示与事实不符）。
    const justFinished = next.some((task) => {
      const before = lastStates[task.id];
      const wasActive = before === "queued" || before === "running";
      const isFinished = task.state === "done" || task.state === "failed";
      return wasActive && isFinished;
    });
    lastStates = Object.fromEntries(next.map((task) => [task.id, task.state]));
    tasks.value = next;

    if (justFinished) {
      emit("finished");
    }
  } catch {
    // 取任务表失败不该打扰用户：下一轮会再试
  }
}

function dismiss() {
  if (lastFinished.value) {
    hidden.value = [...hidden.value, lastFinished.value.id];
  }
}

/** 结束的任务多了会越留越久：成功的自动消失，失败的留着等人看 */
function prune() {
  const now = Date.now();
  const fresh: number[] = [];
  for (const task of tasks.value) {
    if (task.state === "failed" || !task.finished_at) {
      continue;
    }
    const at = new Date(task.finished_at).getTime();
    if (!Number.isNaN(at) && now - at < 8000) {
      continue;
    }
    fresh.push(task.id);
  }
  if (fresh.length > 0) {
    hidden.value = [...new Set([...hidden.value, ...fresh])];
  }
}

onMounted(() => {
  void poll();
  timer = window.setInterval(() => {
    void poll().then(prune);
  }, 800);
});

onBeforeUnmount(() => window.clearInterval(timer));
</script>

<template>
  <div v-if="show" class="taskbar">
    <template v-if="runningTask">
      <span class="taskbar__spinner" aria-hidden="true" />
      <span class="taskbar__label">{{ runningTask.label }}</span>
      <span class="taskbar__state">正在执行…</span>
      <span v-if="active.length > 1" class="taskbar__count">
        另有 {{ active.length - 1 }} 项排队
      </span>
    </template>

    <template v-else-if="lastFinished">
      <span
        class="taskbar__mark"
        :class="{ 'taskbar__mark--bad': lastFinished.state === 'failed' }"
        aria-hidden="true"
      >{{ lastFinished.state === "failed" ? "✕" : "✓" }}</span>
      <span class="taskbar__label">{{ lastFinished.label }}</span>
      <span class="taskbar__message">{{ lastFinished.message }}</span>
      <button class="taskbar__close" type="button" @click="dismiss">关闭</button>
    </template>
  </div>
</template>

<style scoped>
/*
 * 贴在左下角：右下角是那组悬浮工具，两者不该叠在一起。
 */
.taskbar {
  position: fixed;
  left: 12px;
  bottom: 12px;
  z-index: 40;
  display: flex;
  align-items: center;
  gap: 10px;
  max-width: min(620px, calc(100vw - 120px));
  padding: 8px 12px;
  border: 1px solid var(--border);
  border-radius: 8px;
  background: var(--surface);
  box-shadow: 0 10px 26px rgb(0 0 0 / 35%);
  font-size: 13px;
}

.taskbar__spinner {
  width: 12px;
  height: 12px;
  flex-shrink: 0;
  border: 2px solid var(--border);
  border-top-color: var(--accent);
  border-radius: 50%;
  animation: taskbar-spin 800ms linear infinite;
}

@keyframes taskbar-spin {
  to {
    transform: rotate(360deg);
  }
}

@media (prefers-reduced-motion: reduce) {
  .taskbar__spinner {
    animation: none;
  }
}

.taskbar__mark {
  flex-shrink: 0;
  color: var(--accent);
}

.taskbar__mark--bad {
  color: var(--link-missing);
}

.taskbar__label {
  flex-shrink: 0;
  color: var(--text);
}

.taskbar__state,
.taskbar__count,
.taskbar__message {
  color: var(--text-dim);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.taskbar__close {
  flex-shrink: 0;
  margin-left: auto;
  padding: 2px 8px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background-color: transparent;
  color: var(--text-dim);
  font-size: 12px;
  cursor: pointer;
}

.taskbar__close:hover {
  background-color: var(--hover);
  color: var(--text);
}
</style>
