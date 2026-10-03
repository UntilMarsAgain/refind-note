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
 * 键位表 —— 与 Rust 侧 `src-tauri/src/keymap.rs` 一一对应。
 *
 * 单独一个文件（`settings/keymap.json`），不进 `preferences.json`：理由写在
 * `keymap.rs` 的模块抬头里。
 */

/**
 * 一条键位：按下的键按顺序排，**修饰键在前**（`["Ctrl","Shift","T"]`）。
 *
 * 存的是键本身，不是 `"Ctrl+Shift+T"` 那种字符串 —— 捕捉按键时手里就是一串
 * 独立的键，存成同一形状，识别与显示都不用去拆字符串。
 *
 * 修饰键一律写作 `Ctrl` / `Shift` / `Alt` / `Meta` / `CapsLock` / `Fn`。
 * **macOS 上的 Command 也存成 `Meta`**，界面显示成 `Cmd` 由 `core/keymap.ts` 决定。
 */
export type Binding = string[];

/**
 * 键位表：动作 id → 键位，**摊平**成一层（Rust 侧是 `#[serde(flatten)]`）。
 *
 * 摊平是为了手改友好：`{"find": ["Ctrl","F"]}` 一眼看得出是什么，
 * 而嵌一层会变成 `{"bindings": {"find": [...]}}`，多一级缩进却不多一点意思。
 *
 * 是**稀疏**的 —— 这里没有的动作表示"用出厂的"。
 */
export type Keymap = Record<string, Binding>;