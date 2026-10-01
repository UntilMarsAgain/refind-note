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
    /** 菜单里的分组 */
    group: string;
}

/** 菜单分组顺序；没登记的页面落到「其它」 */
export const SPECIAL_GROUPS = ["导航", "工具", "维护"] as const;

export const FALLBACK_GROUP = "其它";

const META: Record<string, SpecialPageMeta> = {
    all: { label: "全部页面", tip: "列出所有笔记", icon: List, group: "导航" },
    random: { label: "随机条目", tip: "随机跳到一篇笔记", icon: Dices, group: "导航" },
    changes: {
        label: "最近编辑",
        tip: "全仓库最近的提交；勾上后连草稿一起看",
        icon: Clock,
        group: "导航",
    },
    history: {
        label: "浏览历史",
        tip: "看过的页面；可单独清空，也能在设置里关掉",
        icon: History,
        group: "导航",
    },
    newtab: { label: "新标签页", tip: "打开一个空白标签页", icon: Plus, group: "导航" },
    settings: { label: "设置", tip: "外观与界面偏好", icon: Settings, group: "工具" },
    keys: {
        label: "GPG 密钥",
        tip: "看本机钥匙串里的钥匙，挑一把当默认",
        icon: KeyRound,
        group: "工具",
    },
    debug: { label: "诊断", tip: "工作目录与数据库的现状", icon: Stethoscope, group: "工具" },
    files: {
        label: "文件",
        tip: "浏览、上传、管理附件",
        icon: FolderOpen,
        group: "导航",
    },
    trash: { label: "回收站", tip: "删过的笔记，可以还原或永久清除", icon: Trash2, group: "维护" },
    gc: { label: "仓库整理", tip: "回收没有引用的内容块与草稿槽位", icon: Wrench, group: "维护" },
};

/** 取某个特殊页面的元信息；没登记的给一份兜底（显示原名，进「其它」） */
export function metaOf(page: string): SpecialPageMeta {
    return (
        META[page] ?? {
            label: `special:${page}`,
            tip: "未登记显示名的特殊页面",
            group: FALLBACK_GROUP,
        }
    );
}

/** 只要一句话名字的地方（标签栏标题）用它 */
export function labelOf(page: string): string {
    return metaOf(page).label;
}
