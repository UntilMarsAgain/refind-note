//! 与后端绑定的类型，统一从这里引入。
//!
//! 一个概念一个文件：改后端时通常只动其中一类，分开放才看得出哪个类型属于哪一块。
//! 手写（不生成）—— 后端改了字段就改这里，`pnpm build` 会指出要跟着改的地方。

export type * from "./address";
export type * from "./note";
export type * from "./history";
export type * from "./command";
export type * from "./debug";
export type * from "./settings";
export type * from "./namespace";
export type * from "./trash";
export type * from "./task";
