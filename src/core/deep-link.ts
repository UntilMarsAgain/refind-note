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
 * 把系统递进来的一条深链拆成**地址**。
 *
 * 与 Rust 侧 `platform::deep_link::address_from_argument` 是同一条规矩 ——
 * 那边管命令行参数（Linux / Windows 是"再拉起一次实例、把 URL 当参数给它"），
 * 这边管走事件递进来的（macOS 与 Android 是这条路）。
 *
 * 认的：`refind://Help:首页`、`refind:///Help:首页`
 *
 * 不认的：
 * - `refind://localhost/file/…` —— 那是**程序内部**取字节用的协议（见
 *   `platform::protocol`），不是"要打开哪一页"；
 * - 空地址，以及别的协议。
 */
export function addressFromDeepLink(url: string): string | null {
    const prefix = "refind://";
    const trimmed = url.trim();
    if (trimmed.slice(0, prefix.length).toLowerCase() !== prefix) {
        return null;
    }

    const rest = trimmed.slice(prefix.length).replace(/^\/+/, "").trim();
    if (!rest || rest.startsWith("localhost/")) {
        return null;
    }

    // 命令行/URL 里的中文多半是百分号编码过来的，还它原样（坏了就照原样用）
    try {
        return decodeURIComponent(rest);
    } catch {
        return rest;
    }
}
