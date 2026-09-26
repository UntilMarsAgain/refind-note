/**
 * 附件（上传的图片、文档……）。
 *
 * 与后端 `storage::files` 对应。`name` 是**原始文件名**，笔记里就用它引用
 * （`![名字](名字)` 或 `::image src=名字`）；磁盘上的名字是生成的标识。
 */
export interface FileEntry {
  id: string;
  /** 原始文件名（可以有中文、空格） */
  name: string;
  size: number;
  sha256: string;
  uploaded: string;
  mime: string;
  /** 取文件的地址（程序自己的 `refind:` 方案） */
  url: string;
}
