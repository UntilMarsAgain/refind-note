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
 * GPG 密钥页（`special:keys`）的状态与动作。
 *
 * 三件事：**看得见**（指纹、用户标识、信任程度、能不能签 / 加密、什么时候过期）、
 * **选得中**（设为仓库默认的签名密钥 / 加密密钥）、**管得了**（从文件导入公钥、
 * 删掉不再需要的公钥）。
 *
 * 与模板分开，是因为这三件事里**没有一件是"显示"**：
 * 列表的数据来自后端，默认项落在偏好里（`protection`），导入要过系统文件选择框 ——
 * 模板只负责把这三样摆在什么位置、按钮什么时候灰掉。
 *
 * **私钥不会离开密钥环**：本模块只读公开信息，也不生成密钥。
 *
 * 另有一层与别的页面不同的规矩：**失败**分两处去。
 * `problem` 是"这一页读不出来 / 改不动"（列表上方那块红的），`notice` 是"办成了但
 * 有一件要你知道的事"（导入带了私钥 —— 那是提醒，不是错误，所以用主题色那一块）。
 */

import { onMounted, ref, type Ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { flash } from "../core/notice.ts";
import { protection, setProtection } from "../core/preferences.ts";
import { shortFingerprint, type GpgKey } from "../ipc/keys.ts";

/** 导入结果（与 `src-tauri` 侧 `import_gpg_key` 的返回一一对应） */
interface ImportSummary {
    imported: number;
    unchanged: number;
    secret_imported: number;
    secret_unchanged: number;
}

/**
 * 两个"设为默认"的差别只在**说给用户听的那句话**上，机制完全一样：
 * 写入 `protection` 里对应的那一个字段，同一把再点一次就是取消（回到"不用"）。
 * 所以措辞与字段名都摆在这张表里，逻辑只有一份。
 */
const DEFAULTS = {
    sign: {
        field: "gpg_sign",
        /** 已取消时的提示 */
        off: "已取消默认签名",
        /** 设为默认时的提示 */
        on: (short: string) => `此后新建笔记默认使用 ${short} 签名`,
        /** 失败时的提示（`: ` 跟在后面） */
        failed: "改默认签名密钥失败：",
    },
    encrypt: {
        field: "gpg_encrypt",
        off: "已取消默认加密",
        on: (short: string) => `此后新建笔记默认加密至 ${short}`,
        failed: "改默认加密密钥失败：",
    },
} as const;

/**
 * 指纹太长，一行里塞不下：`shortFingerprint` 只留首尾，要整条的那一份在列表里
 * 那一行（那一行本身是可选的文字，可以整条复制走）。
 */
const short = shortFingerprint;

/** 时间戳显示成人读的日期；读不出来（空串 / 不是 ISO）就把原样退回 */
export function when(at: string): string {
    if (!at) {
        return "";
    }
    const stamp = new Date(at);
    return Number.isNaN(stamp.getTime()) ? at : stamp.toLocaleDateString();
}

/** 过期那一栏：已过期 / 到某天 / 永不过期 */
export function expiresOf(key: GpgKey): string {
    if (key.expired) {
        return "已过期";
    }
    return key.expires ? `到 ${when(key.expires)}` : "永不过期";
}

export interface KeyRing {
    keys: Ref<GpgKey[]>;
    loading: Ref<boolean>;
    busy: Ref<boolean>;
    /** 页面上那块红的：读列表 / 改不动时的原因 */
    problem: Ref<string>;
    /** 正在"确认删除"的那一把（指纹）；两步确认 */
    confirming: Ref<string>;
    /** 导入之后的提醒（带了私钥这种事必须说出来） */
    notice: Ref<string>;

    /** 这一把是不是当前的默认签名 / 加密密钥 */
    isSigning: (key: GpgKey) => boolean;
    isEncrypting: (key: GpgKey) => boolean;
    useForSigning: (key: GpgKey) => Promise<void>;
    useForEncrypting: (key: GpgKey) => Promise<void>;
    importKey: () => Promise<void>;
    /** 两步确认：第一次上膛，第二次真删 */
    remove: (key: GpgKey) => Promise<void>;
}

export function useKeyRing(): KeyRing {
    const keys = ref<GpgKey[]>([]);
    const loading = ref(false);
    const busy = ref(false);
    const problem = ref("");
    const confirming = ref("");
    const notice = ref("");

    async function load() {
        loading.value = true;
        problem.value = "";
        try {
            keys.value = await invoke<GpgKey[]>("gpg_keys");
        } catch (reason) {
            keys.value = [];
            problem.value = String(reason);
        } finally {
            loading.value = false;
        }
    }

    onMounted(() => void load());

    /** 这一把是不是当前的默认签名 / 加密密钥 */
    function isDefault(role: keyof typeof DEFAULTS, key: GpgKey): boolean {
        return protection.value[DEFAULTS[role].field] === key.fingerprint;
    }

    function isSigning(key: GpgKey): boolean {
        return isDefault("sign", key);
    }

    function isEncrypting(key: GpgKey): boolean {
        return isDefault("encrypt", key);
    }

    /** 设为默认：同一样再点一次就是取消（回到"不用"） */
    async function setDefault(role: keyof typeof DEFAULTS, key: GpgKey) {
        const wording = DEFAULTS[role];
        busy.value = true;
        try {
            await setProtection({
                ...protection.value,
                [wording.field]: isDefault(role, key) ? null : key.fingerprint,
            });
            // 注意这里**又问了一次**，而且是在写入之后：`setProtection` 会同步把
            // `protection` 换成新的，所以这一次问到的是写完的值。取消默认时因此
            // 落到 `on(...)` 那一句上（说"此后默认用它"）—— 与拆分之前一模一样，
            // 属于原有行为，不在这次拆分里改。
            flash(
                isDefault(role, key)
                    ? wording.off
                    : wording.on(short(key.fingerprint)),
            );
        } catch (error) {
            flash(`${wording.failed}${error}`);
        } finally {
            busy.value = false;
        }
    }

    function useForSigning(key: GpgKey) {
        return setDefault("sign", key);
    }

    function useForEncrypting(key: GpgKey) {
        return setDefault("encrypt", key);
    }

    async function importKey() {
        // 私钥文件同样能导（gpg 自己分得清）：文件里带了私钥，导完要明说一声
        const picked = await open({
            multiple: false,
            title: "选择要导入的密钥文件（可含私钥）",
            filters: [{ name: "密钥文件", extensions: ["asc", "gpg", "pub", "key", "sec", "skr"] }],
        });
        if (!picked || Array.isArray(picked)) {
            return;
        }

        busy.value = true;
        try {
            const summary = await invoke<ImportSummary>("import_gpg_key", { path: picked });

            const parts = [`新增 ${summary.imported} 个`, `无变化 ${summary.unchanged} 个`];
            if (summary.secret_imported > 0) {
                parts.push(`其中含私钥 ${summary.secret_imported} 个`);
            }
            flash(`导入完成：${parts.join("，")}`);

            // 私钥进来了是件大事：从此这台机器能替那个人签名、解密
            notice.value =
                summary.secret_imported > 0
                    ? `本次导入包含 ${summary.secret_imported} 个私钥，已存入本机密钥环 —— 本机自此可用于其签名与解密。如非必要，请用 gpg 将其删除（本页仅能删除公钥）。`
                    : "";
            await load();
        } catch (error) {
            problem.value = String(error);
        } finally {
            busy.value = false;
        }
    }

    async function remove(key: GpgKey) {
        if (confirming.value !== key.fingerprint) {
            confirming.value = key.fingerprint;
            return;
        }

        busy.value = true;
        try {
            await invoke("delete_gpg_key", { fingerprint: key.fingerprint });
            flash(`已删除公钥 ${short(key.fingerprint)}`);
            confirming.value = "";
            await load();
        } catch (error) {
            problem.value = String(error);
        } finally {
            busy.value = false;
        }
    }

    return {
        keys,
        loading,
        busy,
        problem,
        confirming,
        notice,
        isSigning,
        isEncrypting,
        useForSigning,
        useForEncrypting,
        importKey,
        remove,
    };
}
