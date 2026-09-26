/**
 * 内部链接的右键菜单。
 *
 * 从 `App.vue` 抽出来：它只有"记下点了哪个链接、然后做一件事"这点状态，
 * 与地址、标签页、笔记都没有交集（动作由调用方提供）。
 */
import { ref } from "vue";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";

export function useLinkMenu(options: {
  /** 在新标签页打开某个内部链接目标 */
  openInNewTab: (title: string) => void;
}) {
  /** 菜单目标（坐标来自鼠标事件）；null = 不显示 */
  const linkMenu = ref<{ title: string; x: number; y: number } | null>(null);

  /** 在新标签页打开 */
  function openLinkMenuTarget() {
    const target = linkMenu.value;
    linkMenu.value = null;
    if (target) {
      options.openInNewTab(target.title);
    }
  }

  /** 复制链接目标（地址栏里能直接粘贴这个写法） */
  function copyLinkTarget() {
    const target = linkMenu.value;
    linkMenu.value = null;
    if (target) {
      // 与正文里的复制走同一条路（Tauri 剪贴板插件）
      void writeText(target.title).catch(() => {});
    }
  }

  /** 关掉菜单（导航时用） */
  function closeLinkMenu() {
    linkMenu.value = null;
  }

  return { linkMenu, openLinkMenuTarget, copyLinkTarget, closeLinkMenu };
}
