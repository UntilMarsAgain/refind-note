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
 * 命名空间管理（设置页的一节，地址 `special:settings#namespaces`）的**状态与动作**。
 *
 * 三件事：**建**（表尾的表单）、**改**（行内编辑：别名、站址、改名）、**清 / 删**
 * （两步确认 —— 这两件事都会把页面移进回收站）。
 *
 * 为什么要与模板分开：这三件事共用同一份状态，而且是**互相串着**的 ——
 * 行内编辑的草稿、两道确认的"上膛"标记、新建表单的三个字段，加上"忙 / 错 / 加载中"
 * 三个过程量，摊在模板旁边时，模板里每个 `v-model` 背后是什么只能靠往上翻。
 * 搬到这里之后，模板只剩下"每一格现在显示成什么样"。
 *
 * **写操作的形状**：每一条命令都返回**更新后的整张表**，这里直接替换，不必再拉一次；
 * `act()` 就是那个统一的出入口 —— 一步到位地把"忙 / 错 / 收摊"处理干净。
 * 校验的权威在后端（名字重复、保留名、非法字符都是那边说了算），这里只挡明显的空输入。
 *
 * 本文件也顺带住了那几个"一个命名空间该显示成什么"的纯函数（`titleOf` / `kindOf` /
 * `anchorOf` / `reserved` / `clearable`）：它们与状态无关，却是**这张表的显示规格**，
 * 与 IPC 放在一处才看得全"哪些行不许动、为什么"。
 */

import { computed, onMounted, ref, type Ref } from "vue";
import { invoke } from "@tauri-apps/api/core";
import type { Namespace } from "../ipc/namespace.ts";

/** 四个内置命名空间的标识：它们各自的处置规则都不一样，见下面那几个纯函数 */
const MAIN = "0";
const SPECIAL = "special";
const TEMPLATE = "template";
const HELP = "help";

/** 行内编辑哪一行、编哪个字段（一次只编一处，所以一个就够） */
export type NamespaceField = "aliases" | "site" | "rename";

export interface NamespaceTable {
    items: Ref<Namespace[]>;
    busy: Ref<boolean>;
    error: Ref<string>;
    loading: Ref<boolean>;
    /** 正在行内编辑哪一行、编哪个字段 */
    editing: Ref<{ id: string; field: NamespaceField } | null>;
    /** 别名编辑的草稿（tags） */
    aliasDraft: Ref<string[]>;
    aliasInput: Ref<string>;
    siteDraft: Ref<string>;
    nameDraft: Ref<string>;
    /** 两道确认的破坏性操作：`清空:<标识>` / `删除:<标识>` */
    armed: Ref<string>;
    /** 新建表单 */
    newName: Ref<string>;
    newSite: Ref<string>;
    newKind: Ref<"internal" | "external">;
    canCreate: Ref<boolean>;

    startAliases: (item: Namespace) => void;
    addAlias: () => void;
    dropAlias: (index: number) => void;
    saveAliases: (item: Namespace) => void;
    startSite: (item: Namespace) => void;
    saveSite: (item: Namespace) => void;
    startRename: (item: Namespace) => void;
    saveRename: (item: Namespace) => void;
    emptyNamespace: (item: Namespace) => void;
    deleteNamespace: (item: Namespace) => void;
    create: () => void;
}

/** 行首那个名字：主命名空间没有名字，写成「（主）」 */
export function titleOf(item: Namespace): string {
    return item.name || "（主）";
}

/**
 * 行首那一行里"这是个什么命名空间"的人话。
 *
 * 顺序有讲究：先按**标识**判内置的几个（它们的语义是写死的），再看有没有站址
 * （有站址就是跨站），剩下的两类（模板 / 内容）才按标识兜底。
 */
export function kindOf(item: Namespace): string {
    if (item.id === MAIN) {
        return "主命名空间";
    }
    if (item.id === SPECIAL) {
        return "虚拟命名空间（页面由程序提供）";
    }
    if (item.site) {
        return "跨站命名空间";
    }
    if (item.id === HELP) {
        return "帮助（随程序发布）";
    }
    return item.id === TEMPLATE ? "保留命名空间（模板与样式）" : "内容命名空间";
}

/** 锚点：`special:settings#ns-help` 能定位到这一行 */
export function anchorOf(item: Namespace): string {
    if (item.id === MAIN) {
        return "ns-main";
    }
    if (item.id === SPECIAL) {
        return "ns-special";
    }
    return `ns-${item.name.trim().replace(/\s+/g, "-") || item.id}`;
}

/** 内置的那几个：站址那一列不摆、也没有改名 / 删除按钮（帮 / 特殊除外，见 `clearable`） */
export function reserved(item: Namespace): boolean {
    return item.id === MAIN || item.id === SPECIAL || item.id === TEMPLATE || item.id === HELP;
}

/**
 * 「清空」对帮助页没有意义：它的正文**随程序发布**，仓库里根本没有可清的页面。
 * 特殊页面同理（它连页面都没有）。于是这两个不给清空按钮。
 */
export function clearable(item: Namespace): boolean {
    return item.id !== SPECIAL && item.id !== HELP;
}

export function useNamespaceTable(): NamespaceTable {
    const items = ref<Namespace[]>([]);
    const busy = ref(false);
    const error = ref("");
    const loading = ref(true);

    const editing = ref<{ id: string; field: NamespaceField } | null>(null);
    /** 别名编辑的草稿（tags） */
    const aliasDraft = ref<string[]>([]);
    const aliasInput = ref("");
    const siteDraft = ref("");
    const nameDraft = ref("");

    /** 两道确认的破坏性操作：`清空:<标识>` / `删除:<标识>` */
    const armed = ref("");

    /** 新建表单 */
    const newName = ref("");
    const newSite = ref("");
    const newKind = ref<"internal" | "external">("internal");

    const canCreate = computed(
        () =>
            !busy.value &&
            newName.value.trim().length > 0 &&
            (newKind.value === "internal" || newSite.value.trim().length > 0),
    );

    async function load() {
        loading.value = true;
        try {
            items.value = await invoke<Namespace[]>("namespaces");
        } catch (reason) {
            error.value = String(reason);
        } finally {
            loading.value = false;
        }
    }

    onMounted(() => void load());

    /** 所有写操作都从这里过：一步到位地把"忙/错/刷新/收摊"处理干净 */
    async function act(command: string, args: Record<string, unknown>) {
        busy.value = true;
        error.value = "";
        try {
            items.value = await invoke<Namespace[]>(command, args);
            editing.value = null;
            armed.value = "";
            return true;
        } catch (reason) {
            error.value = String(reason);
            return false;
        } finally {
            busy.value = false;
        }
    }

    // ---------------- 行内编辑 ----------------

    function startAliases(item: Namespace) {
        editing.value = { id: item.id, field: "aliases" };
        aliasDraft.value = [...item.aliases];
        aliasInput.value = "";
    }

    function addAlias() {
        const alias = aliasInput.value.trim();
        if (alias && !aliasDraft.value.some((kept) => kept.toLowerCase() === alias.toLowerCase())) {
            aliasDraft.value.push(alias);
        }
        aliasInput.value = "";
    }

    function dropAlias(index: number) {
        aliasDraft.value.splice(index, 1);
    }

    function saveAliases(item: Namespace) {
        void act("update_namespace", {
            key: item.id,
            aliases: aliasDraft.value,
            site: item.site,
        });
    }

    function startSite(item: Namespace) {
        editing.value = { id: item.id, field: "site" };
        siteDraft.value = item.site ?? "";
    }

    function saveSite(item: Namespace) {
        void act("update_namespace", {
            key: item.id,
            aliases: item.aliases,
            site: siteDraft.value.trim() || null,
        });
    }

    function startRename(item: Namespace) {
        editing.value = { id: item.id, field: "rename" };
        nameDraft.value = item.name;
    }

    function saveRename(item: Namespace) {
        const name = nameDraft.value.trim();
        if (!name || name === item.name) {
            editing.value = null;
            return;
        }
        void act("rename_namespace", { key: item.id, name });
    }

    // ---------------- 清空 / 删除（两步确认） ----------------

    function emptyNamespace(item: Namespace) {
        if (armed.value !== `empty:${item.id}`) {
            armed.value = `empty:${item.id}`;
            return;
        }
        void act("empty_namespace", { key: item.id });
    }

    function deleteNamespace(item: Namespace) {
        if (armed.value !== `delete:${item.id}`) {
            armed.value = `delete:${item.id}`;
            return;
        }
        void act("delete_namespace", { key: item.id });
    }

    // ---------------- 新建 ----------------

    function create() {
        if (!canCreate.value) {
            return;
        }
        void act("add_namespace", {
            name: newName.value.trim(),
            aliases: [],
            site: newKind.value === "external" ? newSite.value.trim() || null : null,
        }).then((ok) => {
            if (ok) {
                newName.value = "";
                newSite.value = "";
                newKind.value = "internal";
            }
        });
    }

    return {
        items,
        busy,
        error,
        loading,
        editing,
        aliasDraft,
        aliasInput,
        siteDraft,
        nameDraft,
        armed,
        newName,
        newSite,
        newKind,
        canCreate,
        startAliases,
        addAlias,
        dropAlias,
        saveAliases,
        startSite,
        saveSite,
        startRename,
        saveRename,
        emptyNamespace,
        deleteNamespace,
        create,
    };
}
