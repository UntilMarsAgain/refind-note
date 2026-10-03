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
 * 特殊页面的元信息：显示名、说明、图标、菜单分组。
 *
 * **只此一处**：顶栏菜单与标签栏标题都从这里取。
 *
 * 哪些页面**存在**由后端说了算（`special_pages` 命令）—— 这里只决定怎么显示。
 * 后端点名了而这里没登记的页面，会落进「其它」并以 `special:<名字>` 出现，
 * 不会因为忘了登记就从菜单里消失。
 */

import type { Component } from "vue";
import { actionById, bindingOf, label } from "./keymap.ts";
import {
    Clock,
    Dices,
    FolderOpen,
    History,
    KeyRound,
    List,
    Plus,
    Settings,
    Stethoscope,
    Trash2,
    Wrench,
} from "@lucide/vue";

export interface SpecialPageMeta {
    /** 显示名 */
    label: string;
    /** 菜单里悬停时的说明 */
    tip: string;
    /** 菜单里的图标；没登记的可以不给 */
    icon?: Component;
    /**
     * 菜单右侧那排小方键：**绑到哪个动作上了**（`"back"` / `"home"` …）。
     *
     * 这里只登记"这个页面对应哪个动作"，**不写键本身** —— 键位是用户能改的，
     * 写死一份在这儿，用户改了键菜单上还显示旧的，等于菜单在骗人。
     * 显示时去 [`./keymap.ts`] 取当前生效的那一串。
     *
     * 没登记的条目就别登记：菜单上写着按不出来，比不写更糟。
     */
    shortcut?: string;
    // 与 `ACTIONS` 里的动作 id 一一对应（见 `core/keymap.ts`）。
    //
    // **目前一个都没填**，这是对的：已配的键（`back` / `forward` / `home` /
    // `menu` / `reload`）作用在**外壳**上，不是"打开某个系统页面" ——
    // 后退不打开浏览历史页，菜单也不打开设置页。硬把它们挂到某个页面上，
    // 菜单上就会写着一个能按的键，而按了去的是别处。
    //
    // 哪天真给某个页面配了快捷键，这里填它的动作 id。
    /** 菜单里的分组 */
    group: string;
}

/** 菜单分组顺序；没登记的页面落到「其它」 */
export const SPECIAL_GROUPS = ["导航", "工具", "维护"] as const;

export const FALLBACK_GROUP = "其它";

const META: Record<string, SpecialPageMeta> = {
    all: { label: "全部页面", tip: "列出全部笔记", icon: List, group: "导航" },
    random: { label: "随机条目", tip: "随机打开一篇笔记", icon: Dices, group: "导航" },
    changes: {
        label: "最近编辑",
        tip: "全部笔记的最近提交，可含草稿",
        icon: Clock,
        group: "导航",
    },
    history: {
        label: "浏览历史",
        tip: "访问过的页面，可单独清空或停用",
        icon: History,
        group: "导航",
    },
    newtab: { label: "新标签页", tip: "新建一个空白标签页", icon: Plus, group: "导航" },
    settings: { label: "设置", tip: "外观、界面与维护选项", icon: Settings, group: "工具" },
    keys: {
        label: "GPG 密钥",
        tip: "查看本机密钥并设为默认",
        icon: KeyRound,
        group: "工具",
    },
    debug: { label: "诊断", tip: "查看数据位置与当前配置", icon: Stethoscope, group: "工具" },
    files: {
        label: "文件",
        tip: "浏览、上传与管理附件",
        icon: FolderOpen,
        group: "导航",
    },
    trash: { label: "回收站", tip: "已删除的笔记，可还原或永久清除", icon: Trash2, group: "维护" },
    gc: { label: "仓库整理", tip: "释放无引用的内容与草稿占用的空间", icon: Wrench, group: "维护" },
};

/** 取某个特殊页面的元信息；没登记的给一份兜底（显示原名，进「其它」） */
export function metaOf(page: string): SpecialPageMeta {
    return (
        META[page] ?? {
            label: `special:${page}`,
            tip: "未登记名称的系统页面",
            group: FALLBACK_GROUP,
        }
    );
}

/**
 * 某个特殊页面在菜单右边该显示哪几个键（**给键盘上印着的字**：`Cmd` / `Option` / `←`）。
 *
 * 返回空数组表示"没登记" —— 与 `shortcut` 那个字段的注释一个意思：
 * 菜单上写着按不出来，比不写更糟。
 */
export function shortcutKeysOf(page: string): string[] {
    const actionId = metaOf(page).shortcut;
    if (!actionId) {
        return [];
    }
    const action = actionById(actionId);
    // 表里没有这个动作 id：那是代码写错了，摆出来只会误导
    return action ? label(bindingOf(action)).split("+") : [];
}

/** 只要一句话名字的地方（标签栏标题）用它 */
export function labelOf(page: string): string {
    return metaOf(page).label;
}
