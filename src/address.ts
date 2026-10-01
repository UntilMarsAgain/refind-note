import { invoke } from "@tauri-apps/api/core";
import type { ParsedAddress } from "./bindings/address.ts";

/**
 * 解析地址栏里的那一行。
 *
 * 空输入不是地址，返回 `null`；语法有问题时，带着一句给人看的话拒绝。
 */
export async function parseAddress(input: string): Promise<ParsedAddress | null> {
    return invoke<ParsedAddress | null>("parse_address", { input });
}

/** 标签栏上显示的名字：主命名空间直接用页面名，其余带上命名空间前缀 */
export function titleOf(parsed: ParsedAddress): string {
    const { namespace, page } = parsed.address;
    return namespace.spelling ? `${namespace.spelling}:${page}` : page;
}
