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

<style scoped src="./rows.css"/>

<!--
  设置页的「快捷键」一节（`#shortcuts`）。

  改键的动作说白了是"按下什么"，所以**没有输入框**：输入框只能打文字，按不出
  `Alt+H`。改成两态的那个按钮 —— 平时显示当前键位，点一下进入"我在听"，再按一下就成。
  想出来用 Esc（不改）或顺手点别处。

  冲突（有人抢同一个键）不当场拦下来，只**标出来**：有些冲突是故意的（比如两个动作
  分属不同上下文），当场拦会让用户以为自己想错了。
-->

<script setup lang="ts">
import { computed, onBeforeUnmount, ref, watch } from "vue";
import { flash } from "../../core/notice.ts";
import {
  ACTIONS,
  SHORTCUT_GROUPS,
  actionsInGroup,
  bindingOf,
  isCustomized,
  label,
  pressedKeys,
  resetBinding,
  setBinding,
  type ShortcutAction,
} from "../../core/keymap.ts";

const props = defineProps<{
  /** 地址里的章节：跳到那一项并高亮（`#shortcuts`） */
  focus?: string;
}>();

/** 正在听键的那一个（同一时刻只有一个在听） */
const listening = ref<string | null>(null);

/**
 * 谁跟谁撞了同一个键。
 *
 * 不当场拦下来，只标出来：有些冲突是**故意**的（两个动作分属不同上下文，
 * 比如"标签页里的"与"页面里的"），当场拦会让用户以为自己按错了。
 *
 * 键位表是稀疏的，但 `bindingOf` 已经把出厂那份合进来了，所以这里比的是
 * "当前真的生效的那一串"。
 */
const claimedBy = computed(() => {
  const out: Record<string, string[]> = {};
  for (const action of ACTIONS) {
    const keys = bindingOf(action).join("+");
    out[action.id] = ACTIONS.filter(
      (other) => other.id !== action.id && bindingOf(other).join("+") === keys,
    ).map((other) => other.label);
  }
  return out;
});

/** 进入"我在听" */
function startListening(action: ShortcutAction) {
  listening.value = action.id;
}

/**
 * 听键时的按键处理。
 *
 * 挂在 `window` 上、且是**捕获**阶段：改键时全局那个快捷键监听也在跑，
 * 不先拦住的话，按 `Alt+T` 会先开一个新标签页。
 */
function onKeydown(event: KeyboardEvent) {
  const id = listening.value;
  if (!id) {
    return;
  }
  // Esc = 不改（不是"把 Esc 绑上去"）
  if (event.key === "Escape") {
    event.preventDefault();
    event.stopPropagation();
    listening.value = null;
    return;
  }

  const keys = pressedKeys(event);
  if (!keys) {
    // 只按了修饰键：等下一个键，别急着结束
    return;
  }

  // 这一行到这里就定了：拦住冒泡，免得顺手触发了别的动作
  event.preventDefault();
  event.stopPropagation();
  listening.value = null;

  const action = ACTIONS.find((a) => a.id === id);
  if (!action) {
    return;
  }
  void apply(action, keys);
}

/** 绑好了：报一句它变成什么 */
async function apply(action: ShortcutAction, keys: string[]) {
  try {
    await setBinding(action, keys);
    flash(`${action.label}：${label(keys)}`);
  } catch (reason) {
    flash(`改键失败：${reason}`);
  }
}

async function restore(action: ShortcutAction) {
  try {
    await resetBinding(action);
  } catch (reason) {
    flash(`恢复默认失败：${reason}`);
  }
}

/**
 * 监听器只在"正在听键"时挂着。
 *
 * 用 `capture`：改键时全局那个快捷键监听也在跑，不先拦住的话，按 `Alt+T`
 * 会顺手开一个新标签页 —— 那正是用户想改掉的那个键。
 */
watch(listening, (id, _previous, onCleanup) => {
  if (!id) {
    return;
  }
  window.addEventListener("keydown", onKeydown, { capture: true });
  onCleanup(() => window.removeEventListener("keydown", onKeydown, { capture: true }));
});
// 组件被卸载时如果还在听键，`watch` 的 cleanup 会跑（Vue 卸载时会停掉 watcher），
// 但显式写一句更让人放心 —— 而且这一句在 Vue 里也确实不该省。
onBeforeUnmount(() => {
  listening.value = null;
});
</script>

<template>
  <h2 class="settings__section">快捷键</h2>

  <p class="settings__note">
    点一个键位再按新的组合键即可改；按 Esc 取消。想改回来就点「恢复默认」。
    键位存在 <code>settings/keymap.json</code>，可以手改。
  </p>

  <div
      id="shortcuts"
      class="row--stack"
      :class="{ 'row--target': props.focus === 'shortcuts' }"
  >
    <div v-for="group in SHORTCUT_GROUPS" :key="group" class="keys__group">
      <h3 class="keys__group-title">{{ group }}</h3>

      <div
          v-for="action in actionsInGroup(group)"
          :key="action.id"
          class="row keys__row"
      >
        <span class="row__label">{{ action.label }}</span>
        <code class="row__id">#{{ action.id }}</code>

        <button
            type="button"
            class="keys__key"
            :class="{ 'keys__key--listening': listening === action.id }"
            @click="startListening(action)"
        >
          {{ listening === action.id ? "按下新的组合键…" : label(bindingOf(action)) }}
        </button>

        <button
            v-if="isCustomized(action)"
            type="button"
            class="ebtn"
            @click="restore(action)"
        >
          恢复默认
        </button>

        <span v-if="claimedBy[action.id]?.length" class="keys__clash">
          与 {{ claimedBy[action.id].join("、") }} 相同
        </span>
      </div>
    </div>
  </div>
</template>

<style scoped>
.keys__group {
  margin-bottom: 14px;
}

.keys__group-title {
  margin: 14px 0 2px;
  color: var(--text-dim);
  font-size: 12.5px;
  font-weight: 500;
}

.keys__key {
  min-width: 132px;
  padding: 5px 12px;
  border: 1px solid var(--border);
  border-radius: 6px;
  background: var(--surface);
  color: var(--text);
  font-family: var(--mono-font);
  font-size: 12.5px;
  text-align: center;
  cursor: pointer;
}

.keys__key:hover {
  border-color: var(--accent-soft);
  background: var(--accent-tint);
}

/* 正在听键：要说清楚"现在按什么都会算数" */
.keys__key--listening {
  border-color: var(--accent);
  background: var(--accent-tint);
  color: var(--text);
  font-family: inherit;
  animation: keys-pulse 1.1s ease-in-out infinite;
}

@keyframes keys-pulse {
  50% {
    opacity: 0.62;
  }
}

@media (prefers-reduced-motion: reduce) {
  .keys__key--listening {
    animation: none;
  }
}

.keys__clash {
  color: var(--danger);
  font-size: 12px;
}
</style>