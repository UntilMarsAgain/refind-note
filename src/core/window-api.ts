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

import { getCurrentWindow, type Window } from "@tauri-apps/api/window";

/**
 * 在Tauri环境下获取Tauri的窗口API，如果不在则返回null。
 *
 * 以便直接在浏览器环境下调试前端样式
 */
export function currentWindow(): Window | null {
    try {
        return getCurrentWindow();
    } catch {
        return null;
    }
}
