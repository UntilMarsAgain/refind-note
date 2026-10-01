/**
 * 这台设备是桌面还是手机 —— 界面偶尔要按它换一条路。
 *
 * 现在只有一处用它：**另存为**。桌面上有系统保存对话框（用户挑位置），
 * 手机上没有那回事（Android 的文件选择器给的是 `content://…`，不是能直接写的路径），
 * 那边统一由后端放进下载目录（见 `platform/saving.rs`）。
 *
 * 答案来自后端（`platform_kind`，编译期就定了），启动时问一次就够 ——
 * 问之前一律当桌面：导出这种事总是人点出来的，那时早就问过了。
 */

import { invoke } from "@tauri-apps/api/core";

let mobile = false;

/** 启动时问一次（`openWorkspace` 那条路上叫） */
export async function refreshPlatform(): Promise<void> {
    try {
        mobile = (await invoke<string>("platform_kind")) === "mobile";
    } catch (error) {
        console.warn("问平台类型失败，按桌面处理：", error);
        mobile = false;
    }
}

/** 手机上吗（没问过、问不到，一律当桌面） */
export function isMobile(): boolean {
    return mobile;
}
