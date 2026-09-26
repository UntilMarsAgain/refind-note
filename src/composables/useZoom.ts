/**
 * Ctrl + 滚轮缩放界面。
 *
 * 用 WebView 自己的缩放（整页等比），而不是逐处改 `font-size` —— 后者要动每一处字号，
 * 而且图片、间距不会跟着变。值存进 `preferences.json`（界面偏好），重启后保持。
 *
 * 立即生效保手感，落盘节流：滚轮一次会连发很多事件，逐个写文件既慢也没意义。
 */
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { effectiveZoom } from "./useAppearance";

const ZOOM_MIN = 0.5;
const ZOOM_MAX = 3;
const ZOOM_STEP = 0.1;
const SAVE_DELAY_MS = 400;

export function useZoom(options: {
  /** 当前缩放（1.0 = 100%，未加载设置为 1） */
  current: () => number;
  /** 立刻生效（内存里的设置也改掉） */
  apply: (zoom: number) => void;
  /** 落盘（这里负责节流） */
  persist: (zoom: number) => void;
}) {
  let saveTimer: number | undefined;

  function onWheel(event: WheelEvent) {
    if (!event.ctrlKey) {
      return;
    }
    // 拦掉 WebView 自己的 Ctrl+滚轮行为，避免两套缩放打架
    event.preventDefault();

    const current = options.current();
    const next = Math.min(
      ZOOM_MAX,
      Math.max(ZOOM_MIN, current - Math.sign(event.deltaY) * ZOOM_STEP),
    );
    if (next === current) {
      return;
    }

    options.apply(next);
    // 与 `applyAppearance` 走同一条换算：这里以前漏了基准，滚一次就把 112% 打回 100%
    void getCurrentWebview().setZoom(effectiveZoom(next));

    window.clearTimeout(saveTimer);
    saveTimer = window.setTimeout(() => options.persist(next), SAVE_DELAY_MS);
  }

  window.addEventListener("wheel", onWheel, { passive: false });
}
