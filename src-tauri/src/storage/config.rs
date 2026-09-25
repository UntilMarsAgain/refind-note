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
