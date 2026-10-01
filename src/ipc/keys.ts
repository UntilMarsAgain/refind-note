/**
 * GPG 密钥 —— 与 Rust 侧 `src-tauri/src/features/keys.rs` 一一对应。
 *
 * 只列钥匙串里本来就有的**公开信息**：私钥不离开钥匙串，本程序也不生成密钥。
 */

export interface GpgKey {
    /** 指纹（全大写十六进制）—— 唯一，但人认不出来 */
    fingerprint: string;
    /** 用户标识（`姓名 <邮箱>`，主标识在前）—— 人认得出的是这个 */
    uids: string[];
    /** 本地对它的信任程度（人话） */
    trust: string;
    /** 有没有私钥（有才签得了、解得开） */
    secret: boolean;
    can_sign: boolean;
    can_encrypt: boolean;
    /** 创建时间；读不出来就是空串 */
    created: string;
    /** 过期时间；空串 = 永不过期 */
    expires: string;
    expired: boolean;
}

/**
 * 指纹太长，界面上只显示首尾。
 *
 * 完整的那一份该在**能复制的地方**（密钥页上那行），一行里塞满 40 个字符
 * 反而看不清谁是谁。
 */
export function shortFingerprint(fingerprint: string): string {
    return fingerprint.length > 16
        ? `${fingerprint.slice(0, 8)}…${fingerprint.slice(-8)}`
        : fingerprint;
}

/** 一把钥匙的显示名：认得出的人名在前，指纹在后兜底 */
export function keyLabel(key: GpgKey): string {
    const who = key.uids[0];
    return who ? `${who} · ${shortFingerprint(key.fingerprint)}` : shortFingerprint(key.fingerprint);
}
