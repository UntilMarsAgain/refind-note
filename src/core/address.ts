/**
 * 地址：**怎么问后端**，以及怎么显示后端给的结果。
 *
 * 后端两步走：
 *
 * - `parse_address` 只做**语法**（前缀、状态词、章节怎么读）；
 * - `resolve_address` 再落到**仓库**上（这一页在不在、是不是特殊页）。
 *
 * 前端只把地址栏那一行递过去，**不自己拼地址字符串** ——
 * 回显用的规范串（`canonical`）是后端给的，所以"语法糖跳转后回显全称"只有一处实现。
 */

import { invoke } from "@tauri-apps/api/core";
import type { ParsedAddress, ResolvedAddress } from "../ipc/address.ts";
import { labelOf } from "./special.ts";

/**
 * 解析地址栏那一行（只做语法；空输入不是地址，返回 `null`）。
 *
 * 解析不了时**以字符串理由 reject**（"没有这个特殊页面：…"），
 * 调用方把它整页显示出来，同时**不动地址栏** —— 用户写错了，就该看见自己写的那一行。
 */
export async function parseAddress(input: string): Promise<ParsedAddress | null> {
    return invoke<ParsedAddress | null>("parse_address", { input });
}

/** 解析并落到仓库上（空输入返回 `null`；`special:random` 会挑一篇落下去） */
export async function resolveAddress(input: string): Promise<ResolvedAddress | null> {
    return invoke<ResolvedAddress | null>("resolve_address", { input });
}

/** 规范地址 —— 地址栏该显示的那一串 */
export function canonicalOf(route: ResolvedAddress | null): string {
    return route?.canonical ?? "";
}

/** 标签栏上显示什么名字 */
export function titleOf(route: ResolvedAddress): string {
    switch (route.outcome.kind) {
        case "note":
        case "missing":
        case "help":
        case "cross-site":
        case "file":
            return route.outcome.title;
        case "special":
            return labelOf(route.outcome.page);
    }
}

/** 地址里带的章节（段落） */
export function sectionOf(route: ResolvedAddress | null): string {
    return route?.address.section ?? "";
}

/**
 * 在规范串上换一个章节：点击正文里的 `#锚点` 时拼**输入**用。
 *
 * 只追加/替换 `#` 之后的部分；前缀（名称、状态）原样保留。
 */
export function withSection(canonical: string, section: string): string {
    const base = canonical.split("#")[0] ?? "";
    return section ? `${base}#${section}` : base;
}
