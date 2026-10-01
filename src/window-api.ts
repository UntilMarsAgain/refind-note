import { getCurrentWindow, type Window } from "@tauri-apps/api/window";

/**
 * 在Tauri环境下获取Tauri的窗口API，如果不在则返回null。
 *
 * 以便直接在浏览器环境下调试前端样式
 */
export function currentWindow(): Window | null {
    try {
        return getCurrentWindow();
    } catch {
        return null;
    }
}
