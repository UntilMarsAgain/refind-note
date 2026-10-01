//! 前端能叫到的那些命令，按**域**分文件。
//!
//! 一个命令只做三件事：开仓库、把参数交给 `vault` / `features`、把结果转出去。
//! 真正的规矩都在下面那几层里 —— 这里不该长出业务逻辑。
//!
//! - [`workspace`]：工作目录、偏好、默认策略；
//! - [`notes`]：笔记的读写与版本；
//! - [`files`]：文件页面；
//! - [`help`] / [`keys`]：帮助页与 GPG 密钥；
//! - [`maintenance`]：回收站与整理；
//! - [`namespaces`]：命名空间表；
//! - [`activity`]：最近更改、浏览历史、特殊页面。

pub mod activity;
pub mod files;
pub mod help;
pub mod keys;
pub mod maintenance;
pub mod namespaces;
pub mod notes;
pub mod workspace;
