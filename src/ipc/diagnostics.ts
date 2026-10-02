/**
 * 诊断页要的那些事实 —— 与 Rust 侧 `commands/diagnostics.rs` 一一对应。
 *
 * 这里**不会有秘密**：同步那一份直接用 `SyncSettings`（它按定义只报"配没配"）。
 */

import type { SyncSettings } from "./sync.ts";

/** 同步现在的样子 */
export interface DiagnosticSync {
    /** 开了、而且该填的都填了 */
    ready: boolean;
    /** 换了钥匙还没重传（下一次同步会把本机整份重传一遍） */
    reupload_pending: boolean;
    settings: SyncSettings;
    /** 记账本里记着几份文件对上了 */
    index_files: number;
    /** 记账本最后一次改动（RFC3339）；还没有就是空串 */
    index_updated: string;
}

/** 仓库里有多少东西（只数文件名与大小） */
export interface DiagnosticRepository {
    /** 事件日志几份 */
    logs: number;
    /** 草稿槽位几个 */
    drafts: number;
    /** 回收站里几条 */
    trash: number;
    /** 内容块几个 */
    blobs: number;
    /** 内容块一共多少字节（磁盘上那份，含封装） */
    blob_bytes: number;
    /** `db/` 整个目录多少字节 */
    database_bytes: number;
}

/** 渲染这一层带了什么 */
export interface DiagnosticRenderer {
    /** 自定义语法 */
    syntax: string[];
    /** 内置模板名 */
    templates: string[];
    /** 帮助页（写死的那三页） */
    help_pages: string[];
}

/** 与系统打交道的那几件事 */
export interface DiagnosticSystem {
    /** `refind://` 注册成什么样了 */
    deep_link: string;
    /** 系统里的 gpg 版本；没有就是空串 */
    gpg_version: string;
}

export interface Diagnostics {
    /** `desktop` / `mobile` */
    platform: string;
    workspace_root: string;
    scratch_dir: string;
    download_dir: string;
    sync: DiagnosticSync;
    repository: DiagnosticRepository;
    renderer: DiagnosticRenderer;
    system: DiagnosticSystem;
}
