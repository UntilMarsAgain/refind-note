/**
 * 设置分两层，别混：
 *
 * - **界面偏好**（主题、正文限宽、标签栏展开）属于「这台机器」，不属于数据，
 *   所以不与笔记一起同步：主题与主题色现在由后端存在 preferences.json，
 *   标签栏展开状态仍留在前端。但键名与读写**只允许出现在这个文件里**，不再散落到各组件里
 *   各写一份 `localStorage.getItem(...)`。
 * - **仓库级设置**（如标题首字母是否大写，会影响数据语义）在 `vault.json` 里，
 *   由后端读写，前端走 `get_settings` / `update_settings` 命令。
 */

/**
 * 界面缩放的**基准**。
 *
 * 默认字号整体偏小，所以把基准定为 112%：用户自己的缩放值是在这个基准**之上**做相对调整。
 *
 * 为什么用"基准 × 缩放"而不是直接把默认值改成 1.12：那样只对新仓库生效，
 * 已经存下缩放值的仓库（包括你自己的）看不到变化 —— 而"默认偏小"是所有仓库共有的问题，
 * 不该靠改各自的存档来解决。
 */
export const BASE_ZOOM = 1.12;

/** 全部界面偏好的键。只认 `1` / `0` 的布尔偏好与字符串偏好混用同一张表。 */
export const PREFERENCE_KEYS = {
  /** 主题模式：system / light / dark。**与 index.html 的防闪烁内联脚本共用**，改这里要同步改它。 */
  theme: "refind-note:theme",
  /** 正文是否限制为阅读栏宽度 */
  limitWidth: "refind-note:limit-width",
  /** 标签栏是否收起（只显示图标） */
  railCollapsed: "refind-note:rail-collapsed",
} as const;

export function readPreference(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    // 存储不可用（隐私模式等）时按「没存过」处理
    return null;
  }
}

export function writePreference(key: string, value: string): void {
  try {
    localStorage.setItem(key, value);
  } catch {
    // 存不下不影响本次会话
  }
}

/** 布尔偏好：只认 `1` / `0`；没存过或值不合法都回落到默认值 */
export function readFlag(key: string, fallback: boolean): boolean {
  const raw = readPreference(key);
  return raw === null ? fallback : raw === "1";
}

export function writeFlag(key: string, value: boolean): void {
  writePreference(key, value ? "1" : "0");
}
