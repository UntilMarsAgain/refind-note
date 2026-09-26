<script setup lang="ts">
/**
 * `special:history` —— 浏览历史。
 *
 * 记的是**界面状态**（存在 localStorage，不占仓库），所以这里可以单独清空，
 * 设置里也可以整个关掉。
 */
import { computed, ref } from "vue";
import { History, Trash2 } from "@lucide/vue";
import { browsingHistory, clearHistory, historyEnabled } from "../history";

const emit = defineEmits<{
  (e: "open", address: string): void;
}>();

/** 正在等第二次确认：清空不可撤销，值得多问一句 */
const confirming = ref(false);

const rows = computed(() =>
  browsingHistory.value.map((entry) => ({
    ...entry,
    when: new Date(entry.at).toLocaleString(),
  })),
);

function clear() {
  clearHistory();
  confirming.value = false;
}
</script>

<template>
  <section class="history">
    <header class="history__head">
      <div class="history__lead">
        <History :size="20" />
        <span>{{ rows.length }} 条记录</span>
      </div>
      <div class="history__actions">
        <template v-if="confirming">
          <button class="history__danger" type="button" @click="clear">确认清空</button>
          <button type="button" @click="confirming = false">取消</button>
        </template>
        <button
          v-else
          class="history__danger"
          type="button"
          :disabled="rows.length === 0"
          @click="confirming = true"
        >
          <Trash2 :size="15" />
          清空历史
        </button>
      </div>
    </header>

    <p v-if="!historyEnabled" class="history__hint">
      浏览历史已在设置里关掉：不再记录新的，已有记录仍然留着，清空由你决定。
    </p>

    <p v-if="rows.length === 0" class="history__empty">
      还没有记录。看过的页面会出现在这里，点一条就能回去。
    </p>

    <ul v-else class="history__list">
      <li v-for="row in rows" :key="row.address + row.at" class="history__item">
        <button class="history__open" type="button" @click="emit('open', row.address)">
          <span class="history__title">{{ row.title }}</span>
          <span class="history__address">{{ row.address }}</span>
        </button>
        <span class="history__when">{{ row.when }}</span>
      </li>
    </ul>
  </section>
</template>

<style scoped>
.history {
  padding: 4px 0 32px;
}

.history__head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
  margin-bottom: 12px;
}

.history__lead {
  display: flex;
  align-items: center;
  gap: 8px;
  color: var(--text-dim);
}

.history__actions {
  display: flex;
  gap: 6px;
}

.history__actions button {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 5px 10px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: var(--bg);
  color: var(--text);
  cursor: pointer;
}

.history__actions button:disabled {
  opacity: 0.5;
  cursor: default;
}

/* 红色只给破坏性操作 */
.history__danger {
  color: var(--danger);
  border-color: var(--danger);
}

.history__hint,
.history__empty {
  color: var(--text-dim);
}

.history__list {
  margin: 0;
  padding: 0;
  list-style: none;
  display: flex;
  flex-direction: column;
}

.history__item {
  display: flex;
  align-items: center;
  gap: 12px;
  border-bottom: 1px solid var(--border);
}

.history__open {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 8px 4px;
  border: 0;
  background: none;
  color: var(--text);
  text-align: left;
  cursor: pointer;
}

.history__open:hover {
  background: var(--accent-tint);
}

.history__title {
  font-weight: 600;
}

.history__address,
.history__when {
  color: var(--text-dim);
  font-size: 0.85em;
}

.history__address {
  word-break: break-all;
}

.history__when {
  flex: none;
}
</style>
