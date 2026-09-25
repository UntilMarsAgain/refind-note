import { computed, readonly, ref } from "vue";
import { PREFERENCE_KEYS, readPreference, writePreference } from "./settings";

/** 三个可选项；"system" 表示跟随操作系统 */
export type ThemeMode = "light" | "dark" | "system";

/** 与 index.html 里的防闪烁内联脚本共用同一个 key（见 settings.ts） */
export const THEME_STORAGE_KEY = PREFERENCE_KEYS.theme;

export const themeOptions: { value: ThemeMode; label: string }[] = [
  { value: "system", label: "跟随系统" },
  { value: "light", label: "浅色" },
  { value: "dark", label: "深色" },
];

function isThemeMode(value: unknown): value is ThemeMode {
  return themeOptions.some((option) => option.value === value);
}

function readStoredMode(): ThemeMode {
  const raw = readPreference(THEME_STORAGE_KEY);
  return isThemeMode(raw) ? raw : "system";
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
  writePreference(THEME_STORAGE_KEY, next);
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

export function cycleThemeMode(): ThemeMode {
  const next = nextOf(mode.value);
  setThemeMode(next);
  return next;
}

export const themeMode = readonly(mode);

/** 系统是否偏好深色（响应式；"跟随系统"时要跟着它变） */
const systemDark = ref(prefersDark.matches);
prefersDark.addEventListener("change", (event) => {
  systemDark.value = event.matches;
});

/** 解析后的实际主题：把"跟随系统"落成一个具体值 */
export const resolvedTheme = computed<"light" | "dark">(() =>
  mode.value === "system" ? (systemDark.value ? "dark" : "light") : mode.value,
);

/**
 * 顶栏与菜单里的图标文件。
 *
 * 外部 SVG 是独立文档，继承不到 `currentColor`，颜色只能写进文件 —— 所以按主题备两张。
 * 这套判断放在这里（而不是某个组件里）：凡是需要这个图标的地方都从这里取，只有一份。
 */
export const logoSrc = computed(() =>
  resolvedTheme.value === "light" ? "/logo-light.svg" : "/logo.svg",
);
