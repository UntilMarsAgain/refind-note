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
 * 存储徽标的**细节那一半**：点开才去问后端的那些事。
 *
 * 与"该显示哪几枚徽章"（`useProtectionLayers.ts`）分开的理由是**贵贱**：
 * 徽章只说明文头里的几个布尔，不解锁就能显示；细节要跑 gpg（验签、解密试探），
 * 是要花时间的活 —— 列一张历史清单时不该替每一行都做一遍，所以**点开才问**，
 * 问过一次就留着（同一版的事实不会变）。
 *
 * 这里是唯一一处 `invoke` 的地方，两条命令：
 * - `protection_report`：这一版的签名 / 加密 / 压缩 / 口令的详细情况；
 * - `passphrase_stored`：这一版的口令此刻还在不在本次会话的内存里。
 *   它只查内存，不像验签那样要跑 gpg，所以**进页面就问**、换一版就重问，不心疼；
 *   它的结果只影响 `symmetric` 那一枚徽章的文案（见 `useProtectionLayers.ts`）。
 *
 * 弹窗的收起规则也在这里：点到别处、Esc 都收 —— 弹窗是看细节用的，不该占着屏幕。
 */

import { computed, onBeforeUnmount, onMounted, ref, watch, type Ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { flash } from "../core/notice.ts";
import { forgetPassphrase } from "../core/preferences.ts";
import { shortFingerprint } from "../ipc/keys.ts";
import type { ProtectionReport } from "../ipc/note.ts";

/**
 * "这是谁"那一栏怎么写。
 *
 * 指纹是唯一的标识，可人认不出来 —— 本机钥匙串里有这一把就写**姓名 <邮箱>**，
 * 指纹缩成首尾跟在后面（要完整的那一份去密钥页抄）；本机没有就只剩指纹，
 * 那时它确实是唯一能说的东西。
 *
 * 缩首尾用 `ipc/keys.ts` 的 `shortFingerprint`：与密钥页是同一套缩法，
 * 两处看到同一个指纹就该长得一模一样。
 */
function whoOf(uid: string | null, key: string): string {
    return uid ? `${uid}（${shortFingerprint(key)}）` : key;
}

/** 弹窗里的一行：左边是名，右边是值 */
export interface ReportRow {
    label: string;
    value: string;
}

export interface ProtectionReportOptions {
    /** 哪一篇：不给就不问后端（也就没有细节可看） */
    title: () => string | undefined;
    /** 哪一版（地址里的版本 token）；不给或 null = 最新版 */
    reference: () => string | null | undefined;
    /** 这一版有没有口令层（决定要不要问"暂存了吗"） */
    symmetric: () => boolean;
    /** 哪一枚徽章点开了（一次只开一个）；由组件持有，弹窗内外都要读它 */
    openedKey: Ref<string | null>;
    /** 组件把根元素绑上来，用于"点到别处就收起" */
    rootEl: Ref<HTMLElement | null>;
}

export interface ProtectionDetails {
    /** 这一版的口令正留在本次会话里（只有它忘得掉） */
    stored: Ref<boolean>;
    /** 弹窗里那几行，按点开的是哪一层来 */
    rows: Ref<ReportRow[]>;
    /** 这一版有没有留着一个"忘掉口令"的出口 */
    holdingPassphrase: Ref<boolean>;
    /** 点开 / 再点一下收起 */
    toggle: (key: string) => void;
    /** 忘掉这一篇的口令（只对 `symmetric` 一层有意义） */
    forget: () => Promise<void>;
}

export function useProtectionReport(options: ProtectionReportOptions): ProtectionDetails {
    const report = ref<ProtectionReport | null>(null);
    const problem = ref("");
    const asking = ref(false);

    async function ask() {
        if (report.value || asking.value || !options.title()) {
            return;
        }
        asking.value = true;
        problem.value = "";
        try {
            report.value = await invoke<ProtectionReport>("protection_report", {
                title: options.title(),
                reference: options.reference() ?? null,
            });
        } catch (error) {
            problem.value = String(error);
        } finally {
            asking.value = false;
        }
    }

    function toggle(key: string) {
        options.openedKey.value = options.openedKey.value === key ? null : key;
        if (options.openedKey.value) {
            void ask();
        }
    }

    const holdingPassphrase = computed(
        () => options.openedKey.value === "symmetric" && report.value?.passphrase_ready === true,
    );

    /**
     * 忘掉这一篇的口令。
     *
     * 口令只存在于本次会话的内存里，"忘掉"即丢弃 —— 再次阅读时需要重新输入。
     */
    async function forget() {
        const title = options.title();
        if (!title) {
            return;
        }
        try {
            await forgetPassphrase(title);
            options.openedKey.value = null;
            report.value = null;
            stored.value = false;
            flash(`已忘掉「${title}」的口令，再次阅读时需要重新输入`);
        } catch (error) {
            problem.value = String(error);
        }
    }

    /**
     * 这一版的口令暂存在本次会话里没有。
     *
     * 单独问、进页面就问：它只查内存，不像验签那样要跑 gpg，所以不心疼。
     */
    const stored = ref(false);

    async function checkStored() {
        if (!options.symmetric() || !options.title()) {
            stored.value = false;
            return;
        }
        try {
            stored.value = await invoke<boolean>("passphrase_stored", {
                title: options.title(),
                reference: options.reference() ?? null,
            });
        } catch {
            stored.value = false;
        }
    }

    watch(
        () => `${options.title() ?? ""}|${options.reference() ?? ""}|${options.symmetric()}`,
        () => void checkStored(),
        { immediate: true },
    );

    /** 换了一篇或换了一版：结论作废，重新问 */
    watch(
        () => `${options.title() ?? ""}|${options.reference() ?? ""}`,
        () => {
            report.value = null;
            problem.value = "";
        },
    );

    /** 弹窗里那几行，按点开的是哪一层来 */
    const rows = computed<ReportRow[]>(() => {
        if (asking.value) {
            return [{ label: "正在查验", value: "……" }];
        }
        if (problem.value) {
            return [{ label: "无法查验", value: problem.value }];
        }

        const found = report.value;
        if (!found) {
            return [];
        }

        switch (options.openedKey.value) {
            case "sign": {
                if (found.signature) {
                    return [
                        { label: "校验", value: found.signature.verified ? "签名有效" : "签名无效" },
                        { label: "信任", value: found.signature.trust ?? "未查明" },
                        { label: "签名者", value: whoOf(found.signature.uid, found.signature.key) },
                        { label: "说明", value: found.signature.detail },
                    ];
                }
                return [
                    { label: "校验", value: "无法查验" },
                    { label: "原因", value: found.signature_problem ?? "这一版没有签名" },
                ];
            }
            case "encrypt": {
                if (!found.encryption) {
                    return [{ label: "校验", value: "这一版没有加密" }];
                }
                return [
                    { label: "加密到", value: whoOf(found.encryption.uid, found.encryption.key) },
                    { label: "本机", value: found.encryption.detail },
                ];
            }
            case "compress":
                // 没压过就没有这一行（压缩那一档本来也只在压过时才摆出来）
                return found.compression ? [{ label: "算法", value: found.compression }] : [];
            case "symmetric":
                return [
                    {
                        label: "本次会话",
                        value:
                            found.passphrase_ready === true
                                ? "口令已输入，可直接阅读"
                                : "尚未输入口令，阅读前需要解锁",
                    },
                    // 算法写在头里：同一份仓库里新旧两档可以并存，所以逐份说
                    ...(found.cipher ? [{ label: "算法", value: found.cipher }] : []),
                ];
            default:
                return [];
        }
    });

    /** 点到别处就收起：弹窗是看细节用的，不该占着屏幕 */
    function onDocumentClick(event: MouseEvent) {
        const root = options.rootEl.value;
        if (root && !root.contains(event.target as Node)) {
            options.openedKey.value = null;
        }
    }

    function onKeydown(event: KeyboardEvent) {
        if (event.key === "Escape") {
            options.openedKey.value = null;
        }
    }

    onMounted(() => {
        document.addEventListener("click", onDocumentClick);
        document.addEventListener("keydown", onKeydown);
    });

    onBeforeUnmount(() => {
        document.removeEventListener("click", onDocumentClick);
        document.removeEventListener("keydown", onKeydown);
    });

    return { stored, rows, holdingPassphrase, toggle, forget };
}
