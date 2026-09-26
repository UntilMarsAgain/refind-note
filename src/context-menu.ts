//! 右键菜单的状态。
//!
//! 与查看大图那套同一形状：只有"在哪儿、有哪些项"几个值，不必上事件总线。
//! 菜单本身只挂一份（App 根上），各处只管**给出自己那几项**。
import { ref } from "vue";

export interface MenuItem {
  label: string;
  /** 破坏性操作标红（与项目里别处的规矩一致：红色只给真会丢东西的动作） */
  danger?: boolean;
  run: () => void | Promise<void>;
}

export interface MenuState {
  x: number;
  y: number;
  items: MenuItem[];
}

export const contextMenu = ref<MenuState | null>(null);

/**
 * 在鼠标位置打开菜单。
 *
 * 位置会**贴边收一下**（菜单比剩下的空间宽或高时往里挪），否则靠右点一下，
 * 菜单会有一半在窗口外面 —— 那看起来就像没反应。
 */
export function openMenu(event: MouseEvent, items: MenuItem[]) {
  if (items.length === 0) {
    closeMenu();
    return;
  }
  const width = 208;
  const rowHeight = 32;
  const height = items.length * rowHeight + 12;
  const x = Math.min(event.clientX, Math.max(0, window.innerWidth - width - 8));
  const y = Math.min(event.clientY, Math.max(0, window.innerHeight - height - 8));
  contextMenu.value = { x, y, items };
}

export function closeMenu() {
  contextMenu.value = null;
}
