//   Refind Note is a note-taking software.
//   Copyright (C) 2026 Until Mars Again
//
//   This program is free software: you can redistribute it and/or modify
//   it under the terms of the GNU Affero General Public License as published by
//   the Free Software Foundation, either version 3 of the License, or
//   (at your option) any later version.
//
//   This program is distributed in the hope that it will be useful,
//   but WITHOUT ANY WARRANTY; without even the implied warranty of
//   MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
//   GNU Affero General Public License for more details.
//
//   You should have received a copy of the GNU Affero General Public License
//   along with this program.  If not, see <http://www.gnu.org/licenses/>.

/**
 * 「这次上传怎么存」—— 文件列表页与单个文件页共用的那一栏。
 *
 * 为什么与两页分开：文件进的是与笔记**同一个** blob 仓，所以压缩、签名、加密、
 * 口令一样成立，"这一版怎么存"这件事在两页上是**同一个问题**；把它抽出来，
 * 两页就不会各写一遍 `policy.symmetric ? passphrase || null : null`（那种判断
 * 写错一处不会报错，只会让某一版悄悄没套上口令层）。
 *
 * 为什么它落在 `composables/`：`policy` 与 `passphrase` 是**按页面实例**各存一份的
 * 响应式状态（`StoragePicker` 用两个 `v-model` 收它们），所以要一个函数把这两条
 * ref 造出来 —— 这正是 `composables/` 的那条规矩；它不碰 DOM，也不自己管应用级单例
 * （仓库默认从 `core/preferences.ts` 的 `protection` 读），所以不到 `core/` 去。
 */

import { ref, type Ref } from "vue";
import { protection } from "../core/preferences.ts";
import type { Policy } from "../ipc/note.ts";

export interface FileStorage {
  /** 这一版怎么存：压缩、签名、gpg 加密、口令加密 */
  policy: Ref<Policy>;
  /** 口令层的口令：只活在这次会话里，交给后端之后就不留了 */
  passphrase: Ref<string>;
  /** 这一版要套口令层时才把口令递过去 */
  passphraseFor: (policy: Policy) => string | null;
  /** 二进制通道（`upload_bytes`）那一组请求头 */
  uploadHeaders: (name: string) => Record<string, string>;
}

/**
 * 文件进的是与笔记**同一个** blob 仓，所以压缩、签名、加密、口令一样成立；
 * 开局照仓库默认填好，改了就从这一版起粘住（与笔记里那一栏是同一套）。
 *
 * 两个页面各有各的一栏 —— 列表页那栏管「接下来传的东西」，文件页那栏管
 * 「传新版时这一版怎么存」；所以这里每次调用造一份新的，不共用。
 */
export function useFileStorage(): FileStorage {
  const policy = ref<Policy>({ ...protection.value });
  /** 口令层的口令：只活在这次会话里，交给后端之后就不留了 */
  const passphrase = ref("");

  /** 这一版要套口令层时才把口令递过去 */
  function passphraseFor(current: Policy): string | null {
    return current.symmetric ? passphrase.value || null : null;
  }

  /**
   * 二进制通道那几个参数：名字、存储方式、口令。
   *
   * 这条通道的请求体整个是文件字节，所以其余参数只能走请求头（后端从头上取）；
   * 头里只放得下 ASCII，值一律百分号编码。
   */
  function uploadHeaders(name: string): Record<string, string> {
    const headers: Record<string, string> = { "x-file-name": encodeURIComponent(name) };
    headers["x-protection"] = JSON.stringify(policy.value);
    const secret = passphraseFor(policy.value);
    if (secret) {
      headers["x-passphrase"] = encodeURIComponent(secret);
    }
    return headers;
  }

  return { policy, passphrase, passphraseFor, uploadHeaders };
}