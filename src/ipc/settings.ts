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
 * 偏好与工作目录信息 —— 与 Rust 侧 `src-tauri/src/settings.rs`、`lib.rs` 一一对应。
 */

import type { MaintenanceInfo } from "./maintenance.ts";
import type { Policy } from "./note.ts";

export type ThemeMode = "system" | "light" | "dark";

/** 三档的显示名（两个诊断面板都用这一份，别各写各的） */
export const THEME_LABELS: Record<ThemeMode, string> = {
    system: "跟随系统",
    light: "浅色",
    dark: "深色",
};

/** 界面偏好：这台机器上这个人用着舒服的东西，落盘在 `settings/preferences.json` */
export interface Preferences {
    /** 界面缩放（1.0 = 100%） */
    zoom: number;
    /** 主题色（`#rrggbb`） */
    accent: string;
    theme: ThemeMode;
    /** 渲染区是否限宽 */
    limit_width: boolean;
    /** 标签栏**开局**是否收起（这一次会话里怎么开合不写盘） */
    rail_collapsed: boolean;
    /** 代码块是否显示行号 */
    code_line_numbers: boolean;
    /** 记不记浏览历史（记下来的在 settings/browsing.jsonl，可单独清空） */
    record_history: boolean;
    /** 同时常驻的标签页数（超出的按最近没用先踢） */
    resident_tabs: number;
    /** 星标过的页面（新标签页上那一片） */
    starred: Star[];
}

/** 星标（收藏）的一页 */
export interface Star {
    /** 规范地址：这一页在哪儿 */
    address: string;
    /** 记下来时的标题（地址记不住，界面上显示的是它） */
    title: string;
}

/** 数据库的认领标记与版本 */
export interface DatabaseMeta {
    kind: string;
    version: string;
    /** 建库时间（RFC3339，UTC） */
    created_at: string;
}

/** 启动时读一次的工作目录概览 */
export interface WorkspaceInfo {
    /** 工作目录（给人看的位置） */
    root: string;
    /** 数据库目录 */
    database_root: string;
    meta: DatabaseMeta;
    preferences: Preferences;
    /** 仓库默认的保护策略（新笔记从它出发） */
    protection: Policy;
    /** 这台计算机上有没有 gpg（没有时签名 / 加密不可用） */
    gpg_available: boolean;
    /** 整理设置：回收站留多少天、自动整理隔多少天、上次各是什么时候 */
    maintenance: MaintenanceInfo;
}
