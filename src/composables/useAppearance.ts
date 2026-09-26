/**
 * 外观：主题、主题色、阅读栏宽度、界面缩放的应用。
 *
 * 从 `App.vue` 抽出来：这一块只跟"设置"有关，与地址、标签页、笔记都没有交集。
 */
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { setThemeMode, type ThemeMode } from "../theme";
import { BASE_ZOOM } from "../settings";
import type { VaultSettings } from "../bindings";

/**
 * `#rrggbb` → 低透明度版本。
 *
 * 主题色在设置里是**不透明** hex，而"表头底色"这类淡染需要透明版本；CSS 里没法对
 * 运行时设的变量做混色（且不能假定支持 color-mix），所以在 JS 里算好一个变量。
 */
export function tintOf(hex: string): string {
  const match = /^#([0-9a-fA-F]{6})$/.exec(hex.trim());
  if (!match) {
    return hex;
  }
  const value = Number.parseInt(match[1]!, 16);
  return `rgba(${(value >> 16) & 255}, ${(value >> 8) & 255}, ${value & 255}, 0.16)`;
}

/**
 * 把设置里的外观落到页面上。
 *
 * - 主题：**不在这里落地** —— `theme.ts` 是它唯一的真相（标题栏按钮、`index.html` 的
 *   防闪烁脚本、跟随系统的实时响应都在那儿）。这里只把后端的值交给它；
 * - 主题色：覆盖 `--accent` / `--accent-soft` / `--accent-tint`；
 * - 限宽：覆盖 `--reading-width`；
 * - 缩放：交给 WebView 自己做（整页等比，和浏览器一致）。生效值是"**基准 × 用户值**"：
 *   基准负责把默认字号整体抬高，用户值只做相对调整。
 */
export function applyAppearance(settings: VaultSettings | null) {
  if (!settings) {
    return;
  }

  setThemeMode(settings.theme as ThemeMode);

  const root = document.documentElement.style;
  root.setProperty("--accent", settings.accent);
  root.setProperty("--accent-soft", settings.accent);
  root.setProperty("--accent-tint", tintOf(settings.accent));
  root.setProperty("--reading-width", `${settings.reading_width}px`);

  void getCurrentWebview().setZoom(effectiveZoom(settings.zoom));
}

/** 用户设置里的缩放 → 实际生效的缩放（乘上基准） */
export function effectiveZoom(zoom: number): number {
  return zoom * BASE_ZOOM;
}
