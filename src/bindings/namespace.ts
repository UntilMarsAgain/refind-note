/**
 * 命名空间 —— 与 Rust 侧 `src-tauri/src/namespace.rs` 一一对应。
 *
 * 名称与别名是给人写的（地址里的前缀），`id` 是给仓库用的（目录名、键）——
 * 所以**改名不动任何文件**。
 */

export interface Namespace {
    /** 标识：稳定键。主命名空间是 `0` */
    id: string;
    /** 规范名（地址里的前缀）；主命名空间是空串 */
    name: string;
    /** 别名：也认，回显时用写下来的那一个 */
    aliases: string[];
    /** 页面是否落在本仓库（虚拟与跨站是 false） */
    storable: boolean;
    /** 跨站地址模板，`$1` 是页面名；只有跨站命名空间才有 */
    site: string | null;
}
