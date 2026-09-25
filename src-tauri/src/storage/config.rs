//! 仓库级设置（`vault.json`）与命名空间表（`namespaces.json`）。
//!
//! 这里放的是**影响数据语义**的设置（如首字母是否大写）。
//! 纯界面偏好（主题、限宽、标签栏展开）不属于数据，留在前端。

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
    /// 界面偏好（主题、主题色、限宽）。
    ///
    /// 它**不影响数据语义**，严格说不属于"仓库设置"。之所以仍放在这里，只因一条更实用的
    /// 约定：**所有设置都在 `~/.refind-note/` 下**，找东西只有一处。代价是将来做同步/备份
    /// 时这一节要整体排除。
    #[serde(default)]
    pub appearance: Appearance,
}

/// 界面偏好
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
            appearance: Appearance::default(),
        }
    }
}
