//! 地址解析的结果：与 Rust 端 `storage/api.rs` 的 `Address` / `Via` 一一对应。
//!
//! **判别式就在这里**（手写，不生成）：改了 Rust 的变体或字段，就改这里，
//! 然后让 `pnpm build` 指出要跟着改的分支。
//!
//! 后端**解析到底**：哪篇笔记、哪一版、还是不存在；前端只按 `kind` 分发。
//! 返回的 `title` / `short_id` 都是规范全称，界面直接拿来回显地址栏。

/** 是通过哪条指令来到这一页的（跟过重定向的阅读路径会带上） */
export interface Via {
  /** 来源页面标题；随机跳转时它是**发起随机的页面**，不是目标 */
  from: string;
  /** 是否随机跳转 */
  random: boolean;
}

export type Address =
  | { kind: "empty" }
  | {
      kind: "note";
      title: string;
      address: string;
      /** 是跟某条指令来到这一页的（直接打开时为 null） */
      via: Via | null;
      /** 指令页面 + `@no-command`：正文要包成代码块显示（不执行指令） */
      code_block: boolean;
    }
  | { kind: "edit"; title: string; address: string }
  | { kind: "history"; title: string; address: string }
  | { kind: "delete"; title: string; address: string }
  | {
      kind: "view-version";
      title: string;
      /** 版本号（远小于 2^53，按普通数字处理） */
      rev: number;
      id: string;
      short_id: string;
      address: string;
    }
  | {
      kind: "rollback-confirm";
      title: string;
      rev: number;
      id: string;
      short_id: string;
      address: string;
    }
  | {
      kind: "special";
      page: string;
      address: string;
      /** 是跟某条指令来到这一页的（直接打开时为 null） */
      via: Via | null;
    }
  | { kind: "missing"; title: string; address: string };
