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

//! 前端能叫到的那些命令，按**域**分文件。
//!
//! 一个命令只做三件事：开仓库、把参数交给 `vault` / `features`、把结果转出去。
//! 真正的规矩都在下面那几层里 —— 这里不该长出业务逻辑。
//!
//! - [`workspace`]：工作目录、偏好、默认策略；
//! - [`notes`]：笔记的读写与版本；
//! - [`files`]：文件页面；
//! - [`help`] / [`keys`]：帮助页与 GPG 密钥；
//! - [`lock`]：页内解锁（模板页与文件共用一条）；
//! - [`maintenance`]：回收站与整理；
//! - [`namespaces`]：命名空间表；
//! - [`activity`]：最近更改、浏览历史、特殊页面；
//! - [`sync`]：与 S3 兼容服务的同步；
//! - [`platform`]：这台设备是桌面还是手机（另存为走哪条路靠它）；
//! - [`diagnostics`]：诊断页要的那些事实（只有后端知道的那一半）。

pub mod activity;
pub mod backup;
pub mod diagnostics;
pub mod files;
pub mod help;
pub mod keys;
pub mod lock;
pub mod maintenance;
pub mod namespaces;
pub mod notes;
pub mod platform;
pub mod sync;
pub mod workspace;
