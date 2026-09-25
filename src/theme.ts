import { computed, readonly, ref } from "vue";

/** 三个可选项；"system" 表示跟随操作系统 */
export type ThemeMode = "light" | "dark" | "system";

/** 与 index.html 里的防闪烁内联脚本共用同一个 key，改这里必须同步改那里 */
export const THEME_STORAGE_KEY = "refind-note:theme";

export const themeOptions: { value: ThemeMode; label: string }[] = [
  { value: "system", label: "跟随系统" },
  { value: "light", label: "浅色" },
  { value: "dark", label: "深色" },
];

function isThemeMode(value: unknown): value is ThemeMode {
  return themeOptions.some((option) => option.value === value);
}

function readStoredMode(): ThemeMode {
  try {
    const raw = localStorage.getItem(THEME_STORAGE_KEY);
    return isThemeMode(raw) ? raw : "system";
  } catch {
    // 存储不可用（隐私模式等）时退回跟随系统
    return "system";
  }
}

const mode = ref<ThemeMode>(readStoredMode());

const prefersDark = window.matchMedia("(prefers-color-scheme: dark)");

function resolve(next: ThemeMode): "light" | "dark" {
  if (next !== "system") {
    return next;
  }
  return prefersDark.matches ? "dark" : "light";
}

/**
 * 只在 <html data-theme> 上落地一个具体的 light / dark，
 * CSS 因此不需要写任何 prefers-color-scheme 媒体查询。
 */
function apply() {
  document.documentElement.dataset.theme = resolve(mode.value);
}

export function setThemeMode(next: ThemeMode) {
  mode.value = next;
  try {
    localStorage.setItem(THEME_STORAGE_KEY, next);
  } catch {
    // 存不下不影响本次会话内生效
  }
  apply();
}

export function initTheme() {
  apply();
  // 选「跟随系统」时要实时响应系统的切换
  prefersDark.addEventListener("change", () => {
    if (mode.value === "system") {
      apply();
    }
  });
}

/** 按 themeOptions 的顺序轮换：跟随系统 → 浅色 → 深色 → 跟随系统 */
function nextOf(current: ThemeMode): ThemeMode {
  const index = themeOptions.findIndex((option) => option.value === current);
  // findIndex 返回 -1 时 (0 % n) 仍落在第一项，天然是个安全兜底
  return themeOptions[(index + 1) % themeOptions.length].value;
}

export function cycleThemeMode() {
  setThemeMode(nextOf(mode.value));
}

/** 给循环切换按钮做提示用：让用户能预期点下去会变成什么 */
export const nextThemeMode = computed(() => nextOf(mode.value));

export const themeMode = readonly(mode);
