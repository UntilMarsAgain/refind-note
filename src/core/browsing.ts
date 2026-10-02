//   Refind Note is a note-taking software.
//   Copyright (C) 2026 Until Mars Again
//
//   This program is free software: you can redistribute it and/or modify
//   it under the terms of the GNU Affero General Public License as published by
//   the Free Software Foundation, either version 3 of the License, or
//   (at your option) any later version.
//
//   This program is distributed in the hope that it will be useful,
//   but WITHOUT ANY WARRANTY; without even the implied warranty of
//   MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
//   GNU Affero General Public License for more details.
//
//   You should have received a copy of the GNU Affero General Public License
//   along with this program.  If not, see <http://www.gnu.org/licenses/>.

/**
 * 浏览历史：**我看过哪些页面**。
 *
 * 记在仓库的 `settings/browsing.jsonl` 里（跟着这台机器走，不属于仓库内容），
 * 记不记由偏好里的开关说了算 —— 关掉只是不再记新的，已经记下的仍然留着。
 *
 * 记录点只有一处：标签页导航成功之后（见 `tabs.ts`）。
 */

import { ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import type { Visit } from "../ipc/activity.ts";
import { preferences } from "./preferences.ts";

export const browsingHistory = ref<Visit[]>([]);

/** 空白标签页不值得记：它不是一个"看过的地方" */
const BLANK = "special:newtab";

/** 记一次访问。开关关着、或者地址是空白页，就什么都不做。 */
export function recordVisit(address: string, title: string): void {
    if (!preferences.value.record_history || !address || address === BLANK) {
        return;
    }

    void invoke<Visit[]>("record_visit", { address, title })
        .then((visits) => {
            browsingHistory.value = visits;
        })
        .catch((error) => console.warn("记浏览历史失败：", error));
}

export async function loadBrowsing(): Promise<void> {
    try {
        browsingHistory.value = await invoke<Visit[]>("browsing_history");
    } catch (error) {
        console.warn("读浏览历史失败：", error);
        browsingHistory.value = [];
    }
}

export async function clearBrowsing(): Promise<void> {
    await invoke("clear_history");
    browsingHistory.value = [];
}
