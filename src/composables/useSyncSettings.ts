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
 * 同步**设置**的运行时那一份（设置页「云端同步」那一节的全部状态与动作）。
 *
 * 与 [`./sync.ts`] 分开的原因：那边回答的是"**什么时候**同步"（时机、排队、冷却、
 * 启动与关窗那几个钩子，模块级单例、全程序共用）；这里回答的是"**配些什么**" ——
 * 连接信息、私钥草稿、上次同步的结果、这一节自己的错误提示。两边只碰两处：
 * 配好之后 `refreshSyncAvailability`（标题栏那颗"立即同步"该出现了），
 * 以及手动那两颗按钮走 `syncNow`（排队与强制抢锁的语义全在那边）。
 *
 * 为什么是**函数**而不是模块级单例：这份草稿只在设置页活着。
 * 半填的 S3 私钥、粘了一半的密钥串，都是"这一页开着才有"的东西 ——
 * 设置页没打开就不该在内存里留一份没人看、也没人清的副本。
 *
 * 私钥与云端密钥**都不出后端**（见 `ipc/sync.ts`），这里只有"配没配"这两个布尔。
 */

import { onMounted, ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import {
    describeReport,
    type SyncSettings,
    type SyncSettingsPatch,
} from "../ipc/sync.ts";
import type { Cipher } from "../ipc/note.ts";
import { flash } from "../core/notice.ts";
import { isMobile } from "../core/platform.ts";
import { refreshSyncAvailability, syncNow as requestSync } from "../core/sync.ts";

/** 这一节的状态与动作。`loadSync` 由 `onMounted` 自己叫，不用调用方记得 */
export function useSyncSettings() {
    /**
     * 同步的设置。
     *
     * 它**不进仓库**：`settings/sync.json` 只在这台机器上（里面有 S3 的密钥），
     * 所以换台机器要重新填一次 —— 密钥跟着机器走，不跟着数据走。
     */
    const sync = ref<SyncSettings>({
        enabled: false,
        encrypt: false,
        cipher: "aes-256-gcm",
        has_key: false,
        has_secret: false,
        endpoint: "",
        region: "",
        bucket: "",
        prefix: "",
        access_key: "",
    });

    /** 改这一栏时：新填的 S3 私钥。**留空就是不改**（界面本来就拿不到原来那一把） */
    const secretDraft = ref("");
    /** 上一次同步的结果（就在这一页上再说一遍，不必去翻浮条） */
    const lastSync = ref("");
    /** 这一节出的错：写在这一节下面，不弹浮条（人在填表，别把他弹走） */
    const syncProblem = ref("");
    /** 从别的机器抄过来的那一串（粘贴进来；交出去之后这边不留） */
    const pastedKey = ref("");

    onMounted(() => {
        void loadSync();
    });

    async function loadSync(): Promise<void> {
        try {
            sync.value = await invoke<SyncSettings>("sync_settings");
            await refreshSyncAvailability();
        } catch (error) {
            syncProblem.value = String(error);
        }
    }

    /** 存一下（开关、每一栏改动都走它）。私钥只有真的重填了才带上 */
    async function saveSync(): Promise<void> {
        const patch: SyncSettingsPatch = {
            enabled: sync.value.enabled,
            encrypt: sync.value.encrypt,
            cipher: sync.value.cipher,
            endpoint: sync.value.endpoint,
            region: sync.value.region,
            bucket: sync.value.bucket,
            prefix: sync.value.prefix,
            access_key: sync.value.access_key,
            ...(secretDraft.value ? { secret_key: secretDraft.value } : {}),
        };
        try {
            sync.value = await invoke<SyncSettings>("set_sync_settings", { patch });
            secretDraft.value = "";
            syncProblem.value = "";
            // 刚填好桶名/密钥：标题栏那颗"立即同步"该出现了
            await refreshSyncAvailability();
        } catch (error) {
            syncProblem.value = String(error);
        }
    }

    /**
     * 现在同步一次。
     *
     * 会跑一会儿（要遍历本地文件、列云端清单、挨个传），所以按钮上写着"正在同步…"，
     * 进度另外由浮条那一路说 —— 这一页只管结果。
     *
     * `force` 是"不等了"：云端那把锁还热着也**直接抢过来**，不干等它超时。
     * 另一边崩在半路、或者你确定它没在同步时用；它要真在同步，两边就撞上了。
     */
    async function syncNow(force = false): Promise<void> {
        syncProblem.value = "";
        try {
            const report = await requestSync({ force });
            if (report) {
                lastSync.value = describeReport(report);
            }
        } catch (error) {
            syncProblem.value = String(error);
        }
    }

    /**
     * 换云端那一层的算法。
     *
     * 存下去就走（不等着点"立即同步"）：这一栏只影响**往后新传的**东西，
     * 已经传上去的照旧解得开，所以不必重传。
     */
    async function setSyncCipher(event: Event): Promise<void> {
        sync.value.cipher = (event.target as HTMLSelectElement).value as Cipher;
        await saveSync();
    }

    /**
     * 生成一把新的云端密钥。
     *
     * **不显示、也不返回**（版本就在后端）：直播、共享屏幕、随手截图都可能把屏幕上的
     * 东西带出去，而存在本地至少得碰到这台电脑。要带到别的机器上就「导出到文件」。
     *
     * 换钥匙意味着云端那些旧密文解不开了，所以下一次同步会**把本机这份整份重传**。
     */
    async function generateSyncKey(): Promise<void> {
        syncProblem.value = "";
        try {
            sync.value = await invoke<SyncSettings>("sync_generate_key");
            await refreshSyncAvailability();
            flash("已生成密钥（不显示）；要带到别的机器上请「导出到文件」");
        } catch (error) {
            syncProblem.value = String(error);
        }
    }

    /** 用另一台机器上生成的那一串（粘贴进来；交出去之后这边不留） */
    async function usePastedKey(): Promise<void> {
        syncProblem.value = "";
        try {
            sync.value = await invoke<SyncSettings>("sync_set_key", { key: pastedKey.value });
            pastedKey.value = "";
            await refreshSyncAvailability();
            flash(sync.value.has_key ? "已用这把密钥；下次同步会把本机这份整份重传" : "已清掉云端加密");
        } catch (error) {
            syncProblem.value = String(error);
        }
    }

    /**
     * 把密钥**导出成一个文件**（带到别的机器上的那条路）。
     *
     * 内容由后端写，界面既不显示也不经手；走的是系统保存对话框。
     */
    async function exportKey(): Promise<void> {
        syncProblem.value = "";
        try {
            // 手机上不问位置：后端放进下载目录，回来说落在哪
            let target: string | null = null;
            if (!isMobile()) {
                target = await save({
                    title: "导出同步密钥",
                    defaultPath: "refind-note-sync-key.txt",
                });
                if (!target) {
                    return;
                }
            }
            const written = await invoke<string>("sync_export_key", { target });
            flash(`密钥已写到 ${written}；那一份文件就是钥匙，别放会被同步的地方`);
        } catch (error) {
            syncProblem.value = String(error);
        }
    }

    /**
     * 把密钥**复制到剪贴板** —— 导出到文件的近路：不落盘，直接粘到另一台机器上。
     *
     * 同样不经手界面（后端直接写进剪贴板，界面上看不到它）。剪贴板是公开的：
     * 同一个桌面里的程序都读得到，所以粘完记得清掉。
     */
    async function copyKey(): Promise<void> {
        syncProblem.value = "";
        try {
            await invoke("sync_copy_key");
            flash("密钥已复制 —— 粘到另一台机器上，然后记得清掉剪贴板");
        } catch (error) {
            syncProblem.value = String(error);
        }
    }

    return {
        sync,
        secretDraft,
        lastSync,
        syncProblem,
        pastedKey,
        saveSync,
        syncNow,
        setSyncCipher,
        generateSyncKey,
        usePastedKey,
        exportKey,
        copyKey,
    };
}
