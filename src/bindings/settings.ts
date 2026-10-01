/**
 * 偏好与工作目录信息 —— 与 Rust 侧 `src-tauri/src/settings.rs`、`lib.rs` 一一对应。
 */

import type { Policy } from "./note.ts";

export type ThemeMode = "system" | "light" | "dark";

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
}
