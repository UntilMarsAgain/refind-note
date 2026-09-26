/**
 * 与 Rust 端**一一对应**的类型。
 *
 * 集中放在一处，因为这些形状必须与后端一致。以前 `NoteSummary` 在三处各写一份，
 * 其中两份已经漏掉了 `command` 字段 —— 改后端时谁也不会想起另外两份；`VaultSettings`
 * 同样有两份。放在一起之后，"哪份漏了"是看得见的，`pnpm build` 也会把该跟着改的地方指出来。
 *
 * 这里只登记"后端给了什么"；界面自己的类型（菜单项的显示信息、组件的局部状态）留在各自组件里。
 */

/** `NoteSummary`：列表里的一条 */
export interface NoteSummary {
  key: string;
  title: string;
  /** 指令信息；普通页面是 null */
  command: CommandInfo | null;
}

/** `CommandInfo`：一条指令页面的标注，全部来自后端那张指令表 */
export interface CommandInfo {
  /** 短名：`redirect` / `random-redirect` / `unrecognized` */
  kind: string;
  /** 中文名，直接显示 */
  label: string;
  /** 一句说明（悬停提示用） */
  detail: string;
}

/** `Note`：读一篇笔记的结果 */
export interface Note {
  key: string;
  title: string;
  markdown: string;
  html: string;
  rev: number;
  modified: string;
}

/** `LoadOutcome`：目标不存在不算错误，而是交给界面处理 */
export interface LoadOutcome {
  note: Note | null;
  title: string;
  deleted: boolean;
}

/** `Draft`：最新的未提交草稿 */
export interface Draft {
  markdown: string;
  base_rev: number;
  at: string;
  /** 草稿也是链上的一版，所以也有 commit ID —— 预览走的就是它 */
  id: string;
  short_id: string;
}

/** `RevisionContent`：历史里某一版的正文 */
export interface RevisionContent {
  rev: number;
  kind: string;
  at: string;
  title: string;
  markdown: string;
  html: string;
}

/** `RevisionSummary`：历史列表里的一版 */
export interface RevisionSummary {
  rev: number;
  /** create / commit / draft / delete */
  kind: string;
  id: string;
  short_id: string;
  at: string;
  bytes: number;
  delta: number;
  supersedes: number[];
  summary: string | null;
}

/** `DiffLine` */
export interface DiffLine {
  kind: string;
  old_line: number | null;
  new_line: number | null;
  text: string;
}

/** `DiffResult`：两版之间 */
export interface DiffResult {
  from_rev: number;
  to_rev: number;
  from_title: string;
  to_title: string;
  lines: DiffLine[];
  inserted: number;
  deleted: number;
}

/** `VaultSettings`：设置页与外观都读它（仓库级设置 + 外观 + 维护时间） */
export interface VaultSettings {
  root: string;
  format: number;
  capital_links: boolean;
  max_title_bytes: number;
  delta_chain_limit: number;
  trash_keep_days: number;
  gc_interval_days: number;
  last_trash_purge: string;
  last_gc: string;
  theme: string;
  accent: string;
  reading_width: number;
  /** 界面缩放（1.0 = 100%） */
  zoom: number;
}

/** `Namespace`：一个命名空间 */
export interface Namespace {
  id: string;
  name: string;
  aliases: string[];
  /** 可存储：页面落在本仓库；虚拟与跨站的都不是 */
  storable: boolean;
  /** 跨站链接的站点地址模板；null = 普通命名空间 */
  site: string | null;
}

/** `TrashEntry`：回收站里的一条 */
export interface TrashEntry {
  title: string;
  deleted_at: string;
  bytes: number;
  /** `null` = 删除时间读不出来（会列出，但不参与自动清理） */
  days_old: number | null;
}

/** `Task`：后台任务 */
export interface Task {
  id: number;
  label: string;
  state: "queued" | "running" | "done" | "failed";
  submitted_at: string;
  finished_at: string;
  message: string;
}
