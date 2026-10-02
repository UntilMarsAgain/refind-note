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
 * 用户帮助 —— 与 Rust 侧 `src-tauri/src/features/help.rs` 一一对应。
 *
 * 正文是仓库 `help/` 目录下的 markdown，编译时就已经编进程序，运行时读不出来源文件。
 */

export interface HelpPage {
    /** 页面名（地址里写的那一段，例如 `Help:入门` 里的「入门」）—— 就是文件名去掉 `.md` */
    slug: string;
    /** 显示标题（`Help:入门`） */
    display: string;
    /** 正文源码 */
    markdown: string;
    /** 渲染好的 HTML */
    html: string;
}
