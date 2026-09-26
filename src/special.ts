/**
 * 特殊页面的元信息：显示名、说明、图标、菜单分组。
 *
 * **全项目只此一处**：顶栏菜单、`special:all` 的清单、标签页标题都从这里取。
 * 哪些页面**存在**由后端说了算（`special_pages` 命令）—— 这里只决定怎么显示；
 * 后端点名了而这里没登记的页面，会落进「其它」分组并以 `special:<名字>` 出现，
 * 不会因为忘了登记而从界面上消失。
 */
import type { Component } from "vue";
import {
  Clock,
  Dices,
  FolderOpen,
  History,
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
  icon?: Component;
  /** 菜单里的分组 */
  group: string;
}

/** 菜单分组顺序；没登记的页面落到「其它」 */
export const SPECIAL_GROUPS = ["导航", "工具", "维护", "其它"] as const;

export const FALLBACK_GROUP = "其它";

export const SPECIAL_META: Record<string, SpecialPageMeta> = {
  all: { label: "全部页面", tip: "列出所有笔记", icon: List, group: "导航" },
  random: { label: "随机条目", tip: "随机跳到一篇笔记", icon: Dices, group: "导航" },
  newtab: { label: "新标签页", tip: "打开一个空白标签页", icon: Plus, group: "导航" },
  files: {
    label: "文件",
    tip: "浏览、上传、管理附件",
    icon: FolderOpen,
    group: "导航",
  },
  changes: {
    label: "最近更改",
    tip: "全仓库最近的提交；勾选后连草稿一起看",
    icon: Clock,
    group: "导航",
  },
  history: {
    label: "浏览历史",
    tip: "看过的页面；可单独清空，也能在设置里关掉",
    icon: History,
    group: "导航",
  },
  settings: { label: "设置", tip: "外观与存储设置", icon: Settings, group: "工具" },
  debug: {
    label: "诊断",
    tip: "仓库现状、渲染报告与布局实测",
    icon: Stethoscope,
    group: "工具",
  },
  trash: {
    label: "回收站",
    tip: "查看删过的笔记，并清理 30 天前的",
    icon: Trash2,
    group: "维护",
  },
  gc: {
    label: "数据库回收",
    tip: "回收孤立数据块与已作废的草稿",
    icon: Wrench,
    group: "维护",
  },
};

/** 取某个特殊页面的元信息；没登记的给一份兜底（显示原名，进「其它」分组） */
export function metaOf(page: string): SpecialPageMeta {
  return (
    SPECIAL_META[page] ?? {
      label: `special:${page}`,
      tip: "未登记显示名的特殊页面",
      group: FALLBACK_GROUP,
    }
  );
}

/** 只要一句话名字的地方（标签页标题）用它 */
export function labelOf(page: string): string {
  return metaOf(page).label;
}
