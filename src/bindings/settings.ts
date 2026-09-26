//! 仓库设置与外观：与 Rust 端 `VaultSettings` 一一对应。
//!
//! 手写（不生成）：改了 Rust 那边就改这里，然后让 `pnpm build` 指出要跟着改的地方。

export interface VaultSettings {
  root: string;
  /** 数据库模型版本（`大.中.小`）；语义见 src-tauri/src/storage/version.rs */
  model_version: string;
  capital_links: boolean;
  max_title_bytes: number;
  delta_chain_limit: number;
  trash_keep_days: number;
  gc_interval_days: number;
  last_trash_purge: string;
  last_gc: string;
  theme: string;
  accent: string;
  reading_width: number;
  /** 界面缩放（1.0 = 100%） */
  zoom: number;
}

/** `Namespace`：一个命名空间 */
