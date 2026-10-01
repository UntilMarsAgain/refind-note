/**
 * 笔记与阅读相关的结果 —— 与 Rust 侧 `src-tauri/src/notes.rs`、`codec.rs` 一一对应。
 *
 * 字段按 snake_case 原样进 JSON；改了 Rust 那边就要改这里。
 */

/** 一个 blob 落盘时用了哪些层（读的时候照头解，不需要口令） */
export interface Protection {
    compress: boolean;
    /** 签名者密钥标识 */
    sign: string | null;
    /** 加密到的密钥标识 */
    encrypt: string | null;
    /** 是否套了口令对称层 */
    symmetric: boolean;
}

/** 写的时候照它来（读的时候不看它） */
export interface Policy {
    compress: boolean;
    gpg_sign: string | null;
    gpg_encrypt: string | null;
    symmetric: boolean;
}

export interface Note {
    /** 规范键，形如 `0:标题` */
    key: string;
    title: string;
    /** 原样源码 */
    markdown: string;
    /** 阅读用的 HTML。内部链接已经标好红/蓝 */
    html: string;
    rev: number;
    created: string;
    modified: string;
    summary: string | null;
    protection: Protection;
}

/** 列表里的一条 */
export interface NoteSummary {
    key: string;
    title: string;
    rev: number;
    bytes: number;
    created: string;
    modified: string;
}

/** 历史上的一版（只有提交；草稿不进事件链） */
export interface RevisionSummary {
    rev: number;
    at: string;
    bytes: number;
    summary: string | null;
    /** 这一版落盘时用了哪些层 —— 各版可能不同 */
    protection: Protection;
}

/**
 * 交给界面读的结果。
 *
 * 读不到**不是**错误：上了锁时要说得出"需要口令"，
 * 界面才好把人带到输入口令那一页去。
 */
export type Reading =
    | { state: "ready"; note: Note }
    | {
    state: "locked";
    protection: Protection;
    /** 刚才那把口令是错的（这次会话里的已被丢掉）—— 界面据此说"再试一次" */
    wrong_passphrase: boolean;
};

/** 一份草稿 */
export interface Draft {
    markdown: string;
    modified: string;
    protection: Protection;
}


/** 把一份策略说成一句话：压过的 / 签过的 / 加密的 / 口令 / 原样（编辑器"存储："那一栏用） */
export function policyLabel(policy: Policy): string {
    const parts: string[] = [];
    if (policy.compress) {
        parts.push("压缩");
    }
    if (policy.gpg_sign) {
        parts.push("签名");
    }
    if (policy.gpg_encrypt) {
        parts.push("加密");
    }
    if (policy.symmetric) {
        parts.push("口令");
    }
    return parts.length > 0 ? parts.join("+") : "原样";
}
