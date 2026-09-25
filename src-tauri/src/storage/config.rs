//! 三个设置文件，按本性分开放：
//!
//! - `vault.json` —— **影响数据语义**的仓库设置（首字母是否大写、增量链上限…）；
//! - `preferences.json` —— **纯界面偏好**（主题、主题色、限宽），不影响数据语义；
//! - `namespaces.json` —— 命名空间表。
//!
//! 它们都住在 `~/.refind-note/` 下，所以"找设置只有一处"这条约定仍然成立；
//! 但**做同步/备份时 `preferences.json` 应当整体排除** —— 换个机器看同一份笔记，
//! 外观没必要跟着一起搬。

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------- 配置

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultConfig {
    pub format: u32,
    /// 标题首字母是否强制大写（对应 MediaWiki 的 $wgCapitalLinks）
    pub capital_links: bool,
    pub max_title_bytes: usize,
    /// 增量链的长度上限：超过就让下一版退回整份快照，免得读取时一路回放。
    pub delta_chain_limit: usize,
}

/// 界面偏好：独立存在 `preferences.json`。
///
/// 与 `VaultConfig` 分开，是因为它**不影响数据语义** —— 同一份笔记换个主题不该变成
/// "数据变了"。将来做同步/备份时，这个文件整体排除即可。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Appearance {
    /// "system" | "light" | "dark"
    pub theme: String,
    /// 主题色（`#rrggbb`）
    pub accent: String,
    /// 正文限宽（px）
    pub reading_width: u32,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            theme: "system".to_string(),
            accent: "#5b8dd6".to_string(),
            reading_width: 1080,
        }
    }
}

impl Default for VaultConfig {
    fn default() -> Self {
        Self {
            format: 1,
            capital_links: true,
            max_title_bytes: crate::title::MAX_TITLE_BYTES,
            delta_chain_limit: 32,
        }
    }
}
