/**
 * 地址：语法层的解析结果，以及它落到仓库上的结论 ——
 * 与 Rust 侧 `src-tauri/src/address.rs`、`resolve.rs` 一一对应。
 *
 * 字段按 snake_case 原样进 JSON；改了 Rust 那边就要改这里
 * （那边有一条线格式测试钉住形状，改错会先炸）。
 */

/**
 * 命名空间：`id` 是它的身份（主命名空间是 `0`），`spelling` 是回显时用的拼写 ——
 * 用别名访问时保留别名，空串 = 主命名空间（没有前缀）
 */
export interface NamespaceRef {
    id: string;
    spelling: string;
}

/** 浏览状态：`ref` 只是 token，含义由仓库那一层解释；null = 不指版本（最新版） */
export type Mode =
    | { kind: "view"; ref: string | null }
    | { kind: "edit" }
    | { kind: "history" }
    | { kind: "delete" }
    | { kind: "rollback"; ref: string }
    | { kind: "unlock"; ref: string | null };

export interface Address {
    namespace: NamespaceRef;
    /** 页面名。规整过：`_` 视作空格、空白折叠、英文首字母大写 */
    page: string;
    mode: Mode;
    /** 章节（段落）。空串 = 没写 */
    section: string;
}

/** 一次解析的产物：地址本身 + 它的规范串 */
export interface ParsedAddress {
    address: Address;
    /** 规范串：地址栏回显、历史都用它 —— 前端不自己拼地址 */
    canonical: string;
}

/** 地址落到仓库上的结论："这是什么地方" */
export type Outcome =
    | { kind: "note"; title: string }
    | { kind: "missing"; title: string }
    | { kind: "special"; page: string }
    /** 帮助页（虚拟命名空间 `Help`）：页面随程序发布，不在仓库里 */
    | { kind: "help"; page: string; title: string };

/** 地址 + 它落到仓库上的结论（`resolve_address` 的产物） */
export interface ResolvedAddress {
    address: Address;
    canonical: string;
    outcome: Outcome;
}
