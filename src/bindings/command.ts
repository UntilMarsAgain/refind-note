//! 指令页面的标注：与 Rust 端 `CommandInfo` 一一对应（内容来自后端那张指令表）。
//!
//! 手写（不生成）：改了 Rust 那边就改这里，然后让 `pnpm build` 指出要跟着改的地方。

export interface CommandInfo {
  /** 短名：`redirect` / `random-redirect` / `unrecognized` */
  kind: string;
  /** 中文名，直接显示 */
  label: string;
  /** 一句说明 */
  detail: string;
  /** 指令的原文参数（重定向的目标地址 / 随机跳转的命名空间）；没有参数时为空 */
  argument: string;
}

/** `Note`：读一篇笔记的结果 */
