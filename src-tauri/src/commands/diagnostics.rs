//! 诊断页要的那些事实。
//!
//! 界面自己看得见的东西（偏好、主题、缩放）不必来这儿；这里给的是**只有后端知道**的：
//! 这台设备是什么、路径各落在哪、同步配成什么样、仓库里有多少东西、
//! 渲染那一层带着哪些语法、以及"与系统打交道"那几件事成没成。
//!
//! **秘密一律不出现**：同步那一份直接用 [`SyncSettingsView`]（它按定义就不含密钥），
//! 这里也不另开一条能读到密钥的路。

use serde::Serialize;
use tauri::Manager as _;

use crate::features::{help, sync};
use crate::open_database;
use crate::platform;
use crate::vault::database::RepositoryFacts;

/// 一次问全：诊断页要显示与复制的东西
#[derive(Debug, Serialize)]
pub struct Diagnostics {
    /// `desktop` / `mobile`
    pub platform: String,
    /// 仓库根目录（桌面上是 `~/.refind-note`，手机上是程序自己的目录）
    pub workspace_root: String,
    /// 临时文件放哪（手机上不是 `/tmp`）
    pub scratch_dir: String,
    /// "另存为 / 导出"落到哪（手机上不问位置，直接进这里；桌面上是对话框选的）
    pub download_dir: String,
    /// 云端同步
    pub sync: SyncFacts,
    /// 仓库里有多少东西
    pub repository: RepositoryFacts,
    /// 渲染这一层带了什么
    pub renderer: RendererFacts,
    /// 与系统打交道的那几件事
    pub system: SystemFacts,
}

/// 同步现在的样子（**不含任何秘密**）
#[derive(Debug, Serialize)]
pub struct SyncFacts {
    /// 开了、而且该填的都填了
    pub ready: bool,
    /// 换了钥匙还没重传（下一次同步会把本机整份重传一遍）
    pub reupload_pending: bool,
    /// 设置本身（这几栏都不含密钥，只报"配没配"）
    pub settings: sync::SyncSettingsView,
    /// 记账本：对上了几份、什么时候记的
    pub index_files: usize,
    pub index_updated: String,
}

/// 渲染这一层
#[derive(Debug, Serialize)]
pub struct RendererFacts {
    /// 自定义语法（`markdown/syntax/` 下注册了哪几种）
    pub syntax: Vec<String>,
    /// 内置模板名（有几个、叫什么 —— 模板报错时先看这里）
    pub templates: Vec<String>,
    /// 帮助页（写死的那三页）
    pub help_pages: Vec<String>,
}

/// 与系统打交道的那几件事
#[derive(Debug, Serialize)]
pub struct SystemFacts {
    /// `refind://` 的注册情况（Linux 上是我们自己写 .desktop；别的平台由系统管）
    pub deep_link: String,
    /// 系统里的 gpg 是哪一个、什么版本（没有就是空串）
    pub gpg_version: String,
}

#[tauri::command]
pub fn diagnostics(app: tauri::AppHandle) -> Result<Diagnostics, String> {
    let (workspace, database) = open_database()?;
    let settings = sync::settings(&workspace);
    let index = sync::index_facts(&workspace);

    Ok(Diagnostics {
        platform: platform::kind().to_string(),
        workspace_root: workspace.root().display().to_string(),
        scratch_dir: platform::staging::scratch_dir().display().to_string(),
        download_dir: app
            .path()
            .download_dir()
            .map(|dir| dir.display().to_string())
            .unwrap_or_else(|_| "（这台设备上问不到）".to_string()),
        sync: SyncFacts {
            ready: settings.is_ready(),
            reupload_pending: settings.reupload,
            settings: settings.view(),
            index_files: index.files,
            index_updated: index.updated,
        },
        repository: database.facts(),
        renderer: renderer_facts(),
        system: SystemFacts {
            deep_link: platform::deep_link::status(&app),
            gpg_version: crate::storage::codec::gpg_version().unwrap_or_default(),
        },
    })
}

fn renderer_facts() -> RendererFacts {
    RendererFacts {
        syntax: crate::markdown::syntax::names()
            .iter()
            .map(|name| name.to_string())
            .collect(),
        templates: crate::markdown::syntax::template::names()
            .iter()
            .map(|name| name.to_string())
            .collect(),
        help_pages: help::PAGES.iter().map(|page| page.to_string()).collect(),
    }
}
