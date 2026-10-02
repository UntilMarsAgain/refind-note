//   Refind Note is a note-taking software.
//   Copyright (C) 2026 Until Mars Again
//
//   This program is free software: you can redistribute it and/or modify
//   it under the terms of the GNU Affero General Public License as published by
//   the Free Software Foundation, either version 3 of the License, or
//   (at your option) any later version.
//
//   This program is distributed in the hope that it will be useful,
//   but WITHOUT ANY WARRANTY; without even the implied warranty of
//   MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
//   GNU Affero General Public License for more details.
//
//   You should have received a copy of the GNU Affero General Public License
//   along with this program.  If not, see <http://www.gnu.org/licenses/>.

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
    | { kind: "unlock"; ref: string | null }
    /** `@no-command`：这一页是指令页，但不跟跳，照原文看 */
    | { kind: "no-command" };

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
    | { kind: "help"; page: string; title: string }
    /** 跨站命名空间里的页面：本仓库没有它，交给浏览器打开 */
    | { kind: "cross-site"; title: string; url: string }
    /** 文件页面（`File:桥.png`）：正文是字节 */
    | { kind: "file"; title: string };

/** 这一页是被哪条指令带过来的（`$$COMMAND$$` 那一页） */
export interface Via {
    /** 发起跳转的那一页（显示标题） */
    from: string;
    /** 是随机跳转（提示语不写具体名字） */
    random: boolean;
}

/** 地址 + 它落到仓库上的结论（`resolve_address` 的产物） */
export interface ResolvedAddress {
    address: Address;
    canonical: string;
    outcome: Outcome;
    /** 这一页能不能改（由后端说了算，界面照它决定摆哪些按钮） */
    editable: boolean;
    /** 被指令带过来时才有的"从哪儿来" */
    via: Via | null;
}
