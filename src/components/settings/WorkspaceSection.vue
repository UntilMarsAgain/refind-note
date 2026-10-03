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
import { invoke } from "@tauri-apps/api/core";
import { flash } from "../../core/notice.ts";
import NamespaceManager from "../pages/NamespaceManager.vue";
import {
  lockAll,
  maintenance,
  preferences,
  refreshWorkspaceInfo,
  updatePreferences,
} from "../../core/preferences.ts";
import { formatTime } from "../../ipc/maintenance.ts";

/**
 * 设置页的「命名空间」「浏览历史」「口令」与「维护」几节
 * （`#namespaces` / `#browsing-history` / `#passphrase` / `#trash-keep-days` …）。
 *
 * 这一块管的是**这份工作目录本身**：里面摆什么、走过哪儿、留多久、删掉的东西什么时候清。
 * 它与外观那一节（这台机器上的偏好）、与封装策略那一节（往后写的数据长什么样）
 * 都不共用的东西，所以单独成一块。
 *
 * 整理那两个天数有个别扭的地方：它们**不在偏好里，而在仓库里**（跟着数据走），
 * 而且后端一次要把两个都收着 —— 所以 `submitMaintenance` 拿另一个当前的补上，
 * 一次提交两个，绝不单发一个（单发会被另一个的旧值覆盖回去）。
 */
const props = defineProps<{
  /** 地址里的章节：跳到那一项并高亮 */
  focus?: string;
}>();

/** 改整理设置：两个天数一起提交（它俩都在仓库的设置里） */
async function submitMaintenance(patch: { trash?: number; gc?: number }) {
  const trash = patch.trash ?? maintenance.value.trash_keep_days;
  const gc = patch.gc ?? maintenance.value.gc_interval_days;

  try {
    await invoke("set_maintenance", {
      trashKeepDays: Math.max(1, Math.round(trash)),
      gcIntervalDays: Math.max(1, Math.round(gc)),
    });
    await refreshWorkspaceInfo();
  } catch (reason) {
    console.warn("改整理设置失败：", reason);
    flash(String(reason));
  }
}

function submitKeepDays(event: Event) {
  const input = event.target as HTMLInputElement;
  const days = Number(input.value);
  if (!Number.isFinite(days)) {
    input.value = String(maintenance.value.trash_keep_days);
    return;
  }
  void submitMaintenance({ trash: days });
}

function submitGcInterval(event: Event) {
  const input = event.target as HTMLInputElement;
  const days = Number(input.value);
  if (!Number.isFinite(days)) {
    input.value = String(maintenance.value.gc_interval_days);
    return;
  }
  void submitMaintenance({ gc: days });
}

function toggleHistory(event: Event) {
  updatePreferences({ record_history: (event.target as HTMLInputElement).checked });
}

/** 忘掉这次会话里存过的**全部**口令（口令从不落盘，所以这就是"锁上"） */
async function lockEverything() {
  try {
    await lockAll();
    flash("已清除本次会话中的全部口令，阅读加密内容时需重新输入");
  } catch (reason) {
    flash(`锁定失败：${reason}`);
  }
}

function isFocused(id: string): boolean {
  return props.focus === id;
}
</script>

<template>
  <h2 class="settings__section">命名空间</h2>

  <div
    id="namespaces"
    class="row row--stack"
    :class="{ 'row--target': isFocused('namespaces') }"
  >
    <div class="row__head">
      <span class="row__label">命名空间</span>
      <code class="row__id">#namespaces</code>
    </div>
    <NamespaceManager/>
  </div>

  <div
    id="browsing-history"
    class="row"
    :class="{ 'row--target': isFocused('browsing-history') }"
  >
    <span class="row__label">浏览历史</span>
    <code class="row__id">#browsing-history</code>
    <label class="row__check">
      <input
        type="checkbox"
        :checked="preferences.record_history"
        @change="toggleHistory"
      />
      <span>记录访问过的页面（可在浏览历史页单独清空）</span>
    </label>
  </div>

  <div
    id="passphrase"
    class="row"
    :class="{ 'row--target': isFocused('passphrase') }"
  >
    <span class="row__label">口令</span>
    <code class="row__id">#passphrase</code>
    <button type="button" class="ebtn" @click="lockEverything">
      清除本次会话中的全部口令
    </button>
    <span class="row__hint">
      口令不写入磁盘，仅保存在本次会话中。此操作会将其清除（即锁定），
      再次阅读时需重新输入。若只想清除某一篇的口令，可在该页的「口令加密」标记上操作。
    </span>
  </div>

  <h2 class="settings__section">维护</h2>

  <p class="settings__note">
    删除的笔记先进入回收站，超过保留期后由启动时的自动维护清理；
    整理仅释放不再被引用的数据。缩短保留期会使一批条目在下次启动时被清理。
  </p>

  <div
    id="trash-keep-days"
    class="row"
    :class="{ 'row--target': isFocused('trash-keep-days') }"
  >
    <span class="row__label">回收站保留</span>
    <code class="row__id">#trash-keep-days</code>
    <input
      class="num"
      type="number"
      min="1"
      max="3650"
      step="1"
      :value="maintenance.trash_keep_days"
      @change="submitKeepDays"
    />
    <span class="row__unit">天</span>
    <span class="row__hint">上次清理：{{ formatTime(maintenance.last_trash_purge) }}</span>
  </div>

  <div
    id="gc-interval-days"
    class="row"
    :class="{ 'row--target': isFocused('gc-interval-days') }"
  >
    <span class="row__label">自动整理间隔</span>
    <code class="row__id">#gc-interval-days</code>
    <input
      class="num"
      type="number"
      min="1"
      max="3650"
      step="1"
      :value="maintenance.gc_interval_days"
      @change="submitGcInterval"
    />
    <span class="row__unit">天</span>
    <span class="row__hint">上次整理：{{ formatTime(maintenance.last_gc) }}</span>
  </div>
</template>

<style scoped src="./rows.css"/>
