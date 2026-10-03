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
 * 存储徽标：**从 `Protection` 得出该显示什么**。
 *
 * 这是全项目里"几个布尔与两个密钥标识 → 一行字与一枚徽章"最集中的地方，所以单独成文件，
 * 把映射关系整个摆出来（**顺序即显示顺序**，别随手调）：
 *
 * | 层           | 条件（`ipc/note.ts` 的 `Protection`） | 文案            | 颜色 class（`storage-badge.css`） |
 * |--------------|------------------------------------|-----------------|-----------------------------------|
 * | `compress`   | `compress`                         | 已压缩           | 无（压缩不改变"读不读得动"）        |
 * | `sign`       | `sign`（签名者密钥标识）           | 已签名           | `storage__badge--sign`（绿）      |
 * | `encrypt`    | `encrypt`（加密到的密钥标识）      | 已加密           | `storage__badge--encrypt`（蓝）   |
 * | `symmetric`  | `symmetric`（套了口令对称层）     | 口令加密 / **口令已暂存** | `storage__badge--symmetric`（主题色） |
 * | ——           | 一层都没有                          | 原样（单独一枚） | 无                                 |
 *
 * 三条要紧的规矩：
 *
 * 1. **一层都没有时显示「原样」**，而不是什么都不显示 —— "没做任何处理"本身是一条信息，
 *    空白会被读成"徽标没画出来"。
 * 2. 四层的 `reportable` 都是 `true`：压缩那一层点开也有话说（"算法"，见
 *    `useProtectionReport.ts` 里那一档），其余三层是必答的细节。
 * 3. `symmetric` 那一枚的文案**看会话状态**：口令此刻就在内存里就说"口令已暂存"，
 *    因为这一枚回答的是"**现在读得动吗**"，而不是"这一版有没有加密"。
 *    这个状态由 `useProtectionReport.ts` 问后端（`passphrase_stored`）得到，
 *    所以这里是**函数**，不是纯数据。
 *
 * 值全部来自 blob 的**明文头**，所以不解锁就能显示 —— "这一页是加密的"这件事
 * 不该等人输了口令才知道。
 */

import { computed, type ComputedRef, type Ref } from "vue";
import type { Protection } from "../ipc/note.ts";

/** 徽标背后的一层。`key` 同时用来挑颜色 class 与决定点开时问哪几行 */
export type LayerKey = "compress" | "sign" | "encrypt" | "symmetric";

export interface StorageLayer {
    key: LayerKey;
    label: string;
    /** 点开有细节可看 */
    reportable: boolean;
}

/**
 * 一层显示成什么字。
 *
 * 只有 `symmetric` 一处要看别的状态（`passphraseStored`：口令此刻还在不在内存里）。
 */
export function layersOf(protection: Protection, passphraseStored: boolean): StorageLayer[] {
    const found: StorageLayer[] = [];

    if (protection.compress) {
        found.push({ key: "compress", label: "已压缩", reportable: true });
    }
    if (protection.sign) {
        found.push({ key: "sign", label: "已签名", reportable: true });
    }
    if (protection.encrypt) {
        found.push({ key: "encrypt", label: "已加密", reportable: true });
    }
    if (protection.symmetric) {
        // 口令在本次会话里就直接说出来：这一枚回答的是"现在读得动吗"
        found.push({
            key: "symmetric",
            label: passphraseStored ? "口令已暂存" : "口令加密",
            reportable: true,
        });
    }

    return found;
}

/** 一句话的总结（`title` 属性上那一行）：有层就并起来，没有就是"原样" */
export function summaryOf(layers: StorageLayer[]): string {
    return layers.length > 0 ? layers.map((layer) => layer.label).join(" · ") : "原样";
}

export interface ProtectionLayers {
    /** 有哪几层（空 = 原样） */
    layers: ComputedRef<StorageLayer[]>;
    /** 一句话总结 */
    label: ComputedRef<string>;
}

/**
 * 在组件里用一次。
 *
 * 两个入参都是**取值函数**：这一版的 `Protection` 是外部传进来的 props，
 * 口令暂存状态来自另一个 composable（`useProtectionReport`）—— 这里只"问一句要"，
 * 不留副本。
 */
export function useProtectionLayers(
    protection: () => Protection,
    passphraseStored: Ref<boolean>,
): ProtectionLayers {
    const layers = computed(() => layersOf(protection(), passphraseStored.value));
    const label = computed(() => summaryOf(layers.value));
    return { layers, label };
}
