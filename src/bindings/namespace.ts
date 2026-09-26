//! 命名空间：与 Rust 端 `Namespace` 一一对应。
//!
//! 手写（不生成）：改了 Rust 那边就改这里，然后让 `pnpm build` 指出要跟着改的地方。

export interface Namespace {
  id: string;
  name: string;
  aliases: string[];
  /** 可存储：页面落在本仓库；虚拟与跨站的都不是 */
  storable: boolean;
  /** 跨站链接的站点地址模板；null = 普通命名空间 */
  site: string | null;
}

/** `TrashEntry`：回收站里的一条 */
