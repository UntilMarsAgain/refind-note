/**
 * 把设置里的外观落到页面上：主题、主题色、界面缩放。
 *
 * 这里只做"应用"，不碰存哪、也不碰什么时候存 —— 那些是 `preferences.ts` 的事。
 */

import { getCurrentWebview } from "@tauri-apps/api/webview";
import type { Preferences } from "../ipc/settings.ts";
import { accent, applyTheme } from "./theme.ts";

/** 把一份偏好应用到页面上 */
export function applyAppearance(preferences: Preferences): void {
    applyTheme(preferences.theme);
    // 主题色写回 theme.ts 的那个 ref：那几个派生变量（soft / tint / solid）由它统一算
    accent.value = preferences.accent;
    applyZoom(preferences.zoom);
}

/**
 * 界面缩放交给 WebView 自己做：整页等比，文字、图片、间距一起变。
 *
 * 逐处改 `font-size` 的话，得把每一处字号都写一遍，而且图片与间距不会跟着变。
 */
export function applyZoom(zoom: number): void {
    try {
        void getCurrentWebview()
            .setZoom(zoom)
            .catch(() => {
                // 缩放在 Tauri 里才生效；拿不到就算了，不影响别的
            });
    } catch {
        // 不在 Tauri 里（直接开浏览器调样式）时没有这一项
    }
}
