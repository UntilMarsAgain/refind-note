/**
 * 同步 —— 与 Rust 侧 `src-tauri/src/features/sync.rs` 一一对应。
 *
 * 传上去的是**仓库**（`db/**` 与 `settings/repository.json`）；这台机器自己的东西
 * （界面偏好、浏览历史、草稿槽位、同步自己的设置与索引）不进同步 —— 那份名单由
 * 工作目录那一层回答（`storage/workspace.rs` 的 `is_synced`）。
 */

/** S3 那一头的连接信息（与 Rust 侧的 `S3Config` 对应） */
export interface S3Config {
    /** 服务地址，形如 `https://s3.example.com`（不带桶名） */
    endpoint: string;
    /** 区域；多数兼容服务不校验，留空就是 us-east-1 */
    region: string;
    bucket: string;
    /** 桶里的前缀（相当于"同步到哪一层目录"），可以为空 */
    prefix: string;
    access_key: string;
    secret_key: string;
}

/**
 * 同步的设置 —— **界面这一侧看不到秘密**。
 *
 * 云端密钥与 S3 私钥都不出后端：存本地至少得碰到这台电脑，显示出来就不一定了
 * （直播、共享屏幕、随手截图都可能把它带出去）。界面只知道"配没配"。
 */
export interface SyncSettings {
    /** 启动时自动同步一次 */
    enabled: boolean;
    /** 传上去之前要不要再套一层 */
    encrypt: boolean;
    /** 云端密钥配好了没有（**钥匙本身不在这份数据里**） */
    has_key: boolean;
    /** S3 私钥配好了没有 */
    has_secret: boolean;
    endpoint: string;
    region: string;
    bucket: string;
    prefix: string;
    access_key: string;
}

/** 改完交回去的那一份：私钥留空就是不改 */
export interface SyncSettingsPatch {
    enabled: boolean;
    encrypt: boolean;
    endpoint: string;
    region: string;
    bucket: string;
    prefix: string;
    access_key: string;
    /** 只有真的换了才带上；不带就是不改 */
    secret_key?: string;
}

/** 一次同步的结果 */
export interface SyncReport {
    uploaded: number;
    downloaded: number;
    /** 云端跟着删掉的 */
    removed_remote: number;
    /** 本机跟着删掉的（别的机器删过） */
    removed_local: number;
    /** 两边都改过、按时间取了新版的那些 */
    conflicts: { path: string; kept: string }[];
    bytes_up: number;
    bytes_down: number;
}

/** 同步跑到哪儿了（后端每走一步发一条 `sync-progress`） */
export interface SyncProgress {
    /** `lock` / `upload` / `download` / `done` */
    phase: string;
    done: number;
    total: number;
    /** 一句人话，界面上直接显示 */
    text: string;
}

/** 空的连接信息（新填这一页时的起点） */
export function emptyS3(): S3Config {
    return {
        endpoint: "",
        region: "",
        bucket: "",
        prefix: "",
        access_key: "",
        secret_key: "",
    };
}

/** 一次同步做了些什么，说成一句话（界面上的"上次同步"） */
export function describeReport(report: SyncReport): string {
    const parts: string[] = [];
    if (report.uploaded > 0) {
        parts.push(`上传 ${report.uploaded}`);
    }
    if (report.downloaded > 0) {
        parts.push(`下载 ${report.downloaded}`);
    }
    if (report.removed_remote > 0) {
        parts.push(`云端删 ${report.removed_remote}`);
    }
    if (report.removed_local > 0) {
        parts.push(`本机删 ${report.removed_local}`);
    }
    if (parts.length === 0) {
        parts.push("两边一样，没有要动的");
    }
    if (report.conflicts.length > 0) {
        parts.push(`${report.conflicts.length} 个两边都改过（按时间取了新的那版）`);
    }
    return parts.join("，");
}
