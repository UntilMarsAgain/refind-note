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

import {computed, ref, watch} from "vue";
import {ThemeMode} from "../ipc/settings.ts";
/* 两个 logo 是 `public/` 里的静态文件：按路径引用，不走打包器 —— 它们是**图标**，
   参与打包器的哈希改名没有好处（换 logo 就是换那个文件） */
const logoLight = "/logo-light.svg";
const logoDark = "/logo.svg";

// 深浅色主题

const media = window.matchMedia("(prefers-color-scheme: dark)");
export const systemDark = ref(media.matches);
export const resolvedTheme = ref<"light" | "dark">(media.matches ? "dark" : "light");

let currentMode: ThemeMode = "system";

export function applyTheme(mode: ThemeMode): void {
    currentMode = mode;

    const resolved = mode === "system" ? (systemDark.value ? "dark" : "light") : mode;
    resolvedTheme.value = resolved;
    document.documentElement.dataset.theme = resolved;
}

media.addEventListener("change", (event) => {
    systemDark.value = event.matches;
    if (currentMode === "system") {
        applyTheme("system");
    }
});
export const logoSrc = computed(() =>
    resolvedTheme.value === "light" ? logoLight : logoDark,
);

// 主题色

/** 解析 #rgb / #rrggbb 为 [r, g, b] */
export function hexToRgb(hex: string): [number, number, number] {
    let h = hex.replace('#', '').trim()
    if (h.length === 3) {
        h = h.split('').map(c => c + c).join('')
    }
    if (!/^[0-9a-fA-F]{6}$/.test(h)) {
        throw new Error(`无效的颜色值: ${hex}`)
    }
    return [
        parseInt(h.slice(0, 2), 16),
        parseInt(h.slice(2, 4), 16),
        parseInt(h.slice(4, 6), 16),
    ]
}

const clamp = (n: number) => Math.min(255, Math.max(0, Math.round(n)))

/** 与白色混合，ratio 越大越亮（0~1） */
export function lighten(hex: string, ratio: number): string {
    const [r, g, b] = hexToRgb(hex)
    const mix = (c: number) => clamp(c + (255 - c) * ratio)
    return `#${[mix(r), mix(g), mix(b)]
        .map(c => c.toString(16).padStart(2, '0'))
        .join('')}`
}

/** 与黑色混合，ratio 越大越暗（0~1） */
export function darken(hex: string, ratio: number): string {
    const [r, g, b] = hexToRgb(hex)
    const mix = (c: number) => clamp(c * (1 - ratio))
    return `#${[mix(r), mix(g), mix(b)]
        .map(c => c.toString(16).padStart(2, '0'))
        .join('')}`
}

/** 转成 rgba 字符串 */
export function withAlpha(hex: string, alpha: number): string {
    const [r, g, b] = hexToRgb(hex)
    return `rgba(${r}, ${g}, ${b}, ${alpha})`
}

export const accent = ref('#5b8dd6')
export function applyAccent(hex: string) {
    const root = document.documentElement
    root.style.setProperty('--accent', hex)
    root.style.setProperty('--accent-soft', lighten(hex, 0.35))
    root.style.setProperty('--accent-tint', withAlpha(hex, 0.16))
    root.style.setProperty('--accent-solid', darken(hex, 0.65))
}
watch(accent, applyAccent, { immediate: true })