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
 * 回收站与仓库整理 —— 与 Rust 侧 `src-tauri/src/maintenance.rs` 一一对应。
 */

/** 回收站里的一条 */
export interface TrashEntry {
    /** 显示标题（还原时按它找） */
    title: string;
    /** 删除时间（RFC3339）；日志里没有删除标记时是空串 */
    deleted_at: string;
    /** 日志文件自己的体积 */
    bytes: number;
    /** 删了几天了；算不出来就是 null（这种永远不会被自动清掉） */
    days_old: number | null;
}

/** 一次仓库整理的账单 */
export interface GcReport {
    removed_blobs: number;
    freed_bytes: number;
    removed_drafts: number;
}

/** 永久清除回收站条目的账单 */
export interface PurgeReport {
    removed: number;
    freed_bytes: number;
    gc: GcReport;
}

/** 开机自动维护的账单：null = 这一轮不用做 */
export interface MaintenanceReport {
    purged: PurgeReport | null;
    gc: GcReport | null;
}

/** 整理设置（跟着仓库走，在 `settings/repository.json` 里） */
export interface MaintenanceInfo {
    /** 回收站留多少天 */
    trash_keep_days: number;
    /** 自动整理隔多少天 */
    gc_interval_days: number;
    /** 上次清回收站的时间；空串 = 从没做过 */
    last_trash_purge: string;
    /** 上次整理的时间；空串 = 从没做过 */
    last_gc: string;
}

/** 体积写成人看的样子 */
export function formatBytes(bytes: number): string {
    if (bytes < 1024) {
        return `${bytes} 字节`;
    }
    if (bytes < 1024 * 1024) {
        return `${(bytes / 1024).toFixed(1)} KiB`;
    }
    return `${(bytes / (1024 * 1024)).toFixed(1)} MiB`;
}

/** 时间戳写成人看的样子；空串就是"从来没有" */
export function formatTime(at: string): string {
    if (!at) {
        return "从未";
    }
    const when = new Date(at);
    return Number.isNaN(when.getTime()) ? at : when.toLocaleString();
}
