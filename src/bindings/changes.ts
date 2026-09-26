/**
 * 最近更改里的一条（与后端 `storage::api::ChangeEntry` 对应）。
 *
 * 与某一篇笔记的版本历史是同一份数据，只是汇总到了一起 —— "刚才做了什么"是全仓库的问题。
 */
export interface ChangeEntry {
  title: string;
  /** commit / draft / delete */
  kind: string;
  rev: number;
  at: string;
  bytes: number;
  /** 相对上一版的字节增减 */
  delta: number;
  /** 展示用的提交短 id */
  short_id: string;
}
