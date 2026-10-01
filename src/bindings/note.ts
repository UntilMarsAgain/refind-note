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

/** 一页指令的信息（`$$COMMAND$$` 那一页；界面据此提示它会跳到哪） */
export interface CommandInfo {
    /** 短名：`redirect` / `random-redirect` / `unrecognized` */
    kind: string;
    /** 中文名 */
    label: string;
    /** 一句说明 */
    detail: string;
    /** 原文参数（重定向的目标 / 随机的命名空间）；没有参数时为空串 */
    argument: string;
}

export interface Note {
    /** 规范键，形如 `0:标题` */
    key: string;
    /** 这一页是指令页时的指令信息 */
    command: CommandInfo | null;
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

/** 某一版的签名校验报告（没有签名层时后端给 null） */
export interface SignatureReport {
    /** 写这一版时指定的签名者（头里记的那个，不一定等于实际签名的那把） */
    key: string;
    /** 签名验过了没有 */
    verified: boolean;
    /** 本机对签名者公钥的信任程度（人话）；验签没跑起来时是 null */
    trust: string | null;
    /** 人话说明：通过时报指纹，没通过时报原因 */
    detail: string;
}

/** 加密层的现状：加密到谁，本机对付不对付得了 */
export interface EncryptionReport {
    /** 写这一版时指定的加密密钥（头里记的那个） */
    key: string;
    /** 本机有没有对应的私钥 —— 有才解得开 */
    secret: boolean;
    /** 人话说明 */
    detail: string;
}

/**
 * 某一版落盘封装的细节。
 *
 * 三项各自独立：查不动的那项空着（`signature_problem` 说明为什么），其余照报 ——
 * 想知道"这一版能不能解开"的时候，不该因为验不了签名就什么都看不到。
 */
export interface ProtectionReport {
    /** 签名层的校验结论；没有签名层、或验签这步没跑起来时是 null */
    signature: SignatureReport | null;
    /** 签名没报出来的原因（没有 gpg、外层口令没给） */
    signature_problem: string | null;
    /** 加密层：加密到谁、本机有没有那把私钥 */
    encryption: EncryptionReport | null;
    /** 口令层：这次会话里有没有这一版的口令（没套口令层就是 null） */
    passphrase_ready: boolean | null;
}

/** 一份草稿 */
export interface Draft {
    markdown: string;
    modified: string;
    protection: Protection;
}


/** 某一版是怎么存的 → 照它写下一版（不显式改封装时就是它） */
export function policyFrom(protection: Protection): Policy {
    return {
        compress: protection.compress,
        gpg_sign: protection.sign,
        gpg_encrypt: protection.encrypt,
        symmetric: protection.symmetric,
    };
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
