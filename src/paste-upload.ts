//! 把剪贴板里的文件收进仓库。
//!
//! 与"选本机文件"那条路分开：粘贴只有字节、没有路径，所以走的是 `upload_bytes`
//! 的二进制通道，文件名放在请求头里。
import { invoke } from "@tauri-apps/api/core";
import type { FileEntry } from "./bindings";
import { pastedNames } from "./paste-files";

/** 收进一批剪贴板文件，返回收好之后的条目（顺序与传入一致） */
export async function uploadPasted(files: File[]): Promise<FileEntry[]> {
  const names = pastedNames(files, new Date());
  const entries: FileEntry[] = [];
  for (const [index, file] of files.entries()) {
    const bytes = new Uint8Array(await file.arrayBuffer());
    entries.push(
      await invoke<FileEntry>("upload_bytes", bytes, {
        // 请求头只能放 ASCII：与后端取件地址用同一套编解码
        headers: { "x-file-name": encodeURIComponent(names[index]) },
      }),
    );
  }
  return entries;
}
