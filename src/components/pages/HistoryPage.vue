<script setup lang="ts">
import { History, Trash2 } from "@lucide/vue";
import { browsingHistory, clearBrowsing } from "../../core/browsing.ts";
import { flash } from "../../core/notice.ts";
import { preferences } from "../../core/preferences.ts";
import { ref } from "vue";

/**
 * 浏览历史（`special:history`）：**我看过哪些页面**。
 *
 * 与「最近编辑」不是一回事：那边是仓库里发生了什么（谁提交了哪一版），
 * 这边是**我看过什么**。所以它按地址去重、可以单独清空、也能在设置里关掉。
 *
 * 一条记录存的是**当时地址栏里那一串**（含状态与章节），点回去就回到那一刻的页面。
 */
const emit = defineEmits<{
    (e: "navigate", input: string): void;
}>();

/** 清空要两步：这一步不可撤销 */
const confirming = ref(false);
const busy = ref(false);

function when(at: string): string {
    if (!at) {
        return "";
    }
    const stamp = new Date(at);
    return Number.isNaN(stamp.getTime()) ? at : stamp.toLocaleString();
}

async function clear() {
    if (!confirming.value) {
        confirming.value = true;
        return;
    }

    busy.value = true;
    try {
        await clearBrowsing();
        confirming.value = false;
        flash("浏览历史已清空");
    } catch (error) {
        flash(`清空失败：${error}`);
    } finally {
        busy.value = false;
    }
}
</script>

<template>
  <section class="history">
    <div class="history__head">
      <History :size="20" :stroke-width="1.9"/>
      <h1 class="history__title">浏览历史</h1>
      <span class="history__count">{{ browsingHistory.length }} 条记录</span>

      <span class="history__actions">
        <template v-if="confirming">
          <button class="history__danger" type="button" :disabled="busy" @click="clear">
            确认清空
          </button>
          <button class="history__btn" type="button" @click="confirming = false">取消</button>
        </template>
        <button
            v-else
            class="history__danger"
            type="button"
            :disabled="busy || browsingHistory.length === 0"
            @click="clear"
        >
          <Trash2 :size="14" :stroke-width="1.9"/>
          清空历史
        </button>
      </span>
    </div>

    <p v-if="!preferences.record_history" class="history__off">
      浏览历史已在设置里关掉：不再记录新的，已有记录仍然留着，清空由你决定。
    </p>

    <p v-if="browsingHistory.length === 0" class="history__hint">
      还没有记录。看过的页面会出现在这里，点一条就能回去。
    </p>

    <ol v-else class="history__list">
      <li v-for="visit in browsingHistory" :key="visit.address" class="history__item">
        <button
            type="button"
            class="history__open"
            :title="visit.address"
            @click="emit('navigate', visit.address)"
        >
          <span class="history__name">{{ visit.title || visit.address }}</span>
          <span class="history__address">{{ visit.address }}</span>
          <span class="history__at">{{ when(visit.at) }}</span>
        </button>
      </li>
    </ol>
  </section>
</template>

<style scoped>
.history {
  padding-top: 18px;
}

.history__head {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
  align-items: baseline;
}

.history__title {
  margin: 0;
  font-size: 1.7em;
}

.history__count {
  color: var(--text-dim);
  font-size: 13px;
}

.history__actions {
  display: inline-flex;
  gap: 6px;
  margin-left: auto;
}

.history__btn {
  padding: 4px 12px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: transparent;
  color: var(--text-dim);
  font: inherit;
  font-size: 12.5px;
  cursor: pointer;
}

.history__btn:hover {
  border-color: var(--accent-soft);
  color: var(--text);
}

/* 清空是不可撤销的，所以它是这一页上唯一的危险色 */
.history__danger {
  display: inline-flex;
  gap: 5px;
  align-items: center;
  padding: 4px 12px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: transparent;
  color: var(--danger);
  font: inherit;
  font-size: 12.5px;
  cursor: pointer;
}

.history__danger:hover:not(:disabled) {
  border-color: var(--danger);
}

.history__danger:disabled {
  opacity: 0.5;
  cursor: default;
}

.history__off {
  margin: 12px 0 0;
  padding: 8px 12px;
  border-left: 3px solid var(--border);
  color: var(--text-dim);
  font-size: 12.5px;
  line-height: 1.7;
}

.history__hint {
  margin: 16px 0 0;
  color: var(--text-dim);
  font-size: 13.5px;
}

.history__list {
  margin: 16px 0 0;
  padding: 0;
  list-style: none;
  border-top: 1px solid var(--border);
}

.history__item {
  border-bottom: 1px solid var(--border);
}

.history__open {
  display: grid;
  grid-template-columns: minmax(0, 16em) minmax(0, 1fr) auto;
  gap: 12px;
  align-items: baseline;
  width: 100%;
  padding: 8px 8px;
  border: 0;
  background: transparent;
  color: var(--text);
  font: inherit;
  font-size: 13.5px;
  text-align: left;
  cursor: pointer;
}

.history__open:hover {
  background: var(--accent-tint);
}

.history__name {
  font-weight: 600;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.history__address,
.history__at {
  color: var(--text-dim);
  font-size: 12.5px;
}

.history__address {
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
</style>
