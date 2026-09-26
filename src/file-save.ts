//! 另存为：把仓库里的附件（或网上的图片）存到用户选的位置。
//!
//! 三处共用：笔记里图片的右键、文件行的右键、大图查看器上的按钮。
//! 字节一律不走前端 —— 后端直接读仓库那份（或直接下载），前端只负责问路径。
import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import { fileNameOfUrl } from "./file-links";

/**
 * 另存一个**仓库里的**附件。返回存到哪里；用户取消则返回 `null`。
 *
 * `key` 可以是标识，也可以是显示名（后端两个都认）。
 */
export async function saveVaultFile(key: string): Promise<string | null> {
  const target = await save({ defaultPath: key, title: "另存为" });
  if (!target) {
    return null;
  }
  await invoke("export_file", { key, target });
  return target;
}

/**
 * 另存一张**网上的**图片。返回存到哪里；用户取消则返回 `null`。
 *
 * 下载在后端做：前端的 `fetch` 会被跨域拦住，后端没有这个限制 ——
 * 顺带字节也就不必经过前端。
 */
export async function saveRemoteFile(url: string): Promise<string | null> {
  const target = await save({ defaultPath: fileNameOfUrl(url), title: "另存为" });
  if (!target) {
    return null;
  }
  await invoke("export_url", { url, target });
  return target;
}
