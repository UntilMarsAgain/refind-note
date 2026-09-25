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
#[serde(default)]
pub struct VaultConfig {
    pub format: u32,
    /// 标题首字母是否强制大写（对应 MediaWiki 的 $wgCapitalLinks）
    pub capital_links: bool,
    pub max_title_bytes: usize,
    /// 增量链的长度上限：超过就让下一版退回整份快照，免得读取时一路回放。
    pub delta_chain_limit: usize,
    /// 回收站保留天数：更早的条目会在自动清理时被删掉。
    pub trash_keep_days: u64,
    /// 自动回收的间隔天数：距上次回收超过它就再跑一次。
    pub gc_interval_days: u64,
    /// 上次清理回收站的时间（RFC3339；空 = 从未跑过）。
    ///
    /// 与设置放在一起，是因为它描述的是**这份数据**的维护状态，换台机器看同一份笔记时
    /// 也该跟着走（对比 `preferences.json`：那份是"这台机器"的偏好，同步时要排除）。
    pub last_trash_purge: String,
    /// 上次回收的时间（RFC3339；空 = 从未跑过）。
    pub last_gc: String,
}

/// 界面偏好：独立存在 `preferences.json`。
///
/// 与 `VaultConfig` 分开，是因为它**不影响数据语义** —— 同一份笔记换个主题不该变成
/// "数据变了"。将来做同步/备份时，这个文件整体排除即可。
///
/// 容器级的 `#[serde(default)]` 是为**向后兼容**：旧文件里没有后来加的字段（如 zoom）时，
/// 用 `Default` 补齐，而不是让整份设置读失败。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Appearance {
    /// "system" | "light" | "dark"
    pub theme: String,
    /// 主题色（`#rrggbb`）
    pub accent: String,
    /// 正文限宽（px）
    pub reading_width: u32,
    /// 界面缩放（1.0 = 100%）。Ctrl + 滚轮调整
    pub zoom: f64,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            theme: "system".to_string(),
            accent: "#5b8dd6".to_string(),
            reading_width: 1080,
            zoom: 1.0,
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
            trash_keep_days: 30,
            gc_interval_days: 30,
            last_trash_purge: String::new(),
            last_gc: String::new(),
        }
    }
}
