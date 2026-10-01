//! 偏好：工作目录 `settings/preferences.json`。
//!
//! 一份扁平的文件：界面缩放、主题色、深浅色、宽度限制器开不开、标签栏收没收起、
//! 代码块显不显示行号。它们都是「这台机器上这个人用着舒服」的东西，没有谁的语义比谁更重，
//! 所以不分子对象。
//!
//! **前端自己不存状态**：凡是需要留到下次的，都写进这里。

use serde::{Deserialize, Serialize};

use crate::workspace::{read_json, write_json, Workspace};

const PREFERENCES_FILE: &str = "preferences.json";

/// 界面缩放的上下限，与设置页的输入范围一致
const ZOOM_MIN: f64 = 0.5;
const ZOOM_MAX: f64 = 3.0;

/// 主题色的默认值，也是取值不合法时的兜底
const DEFAULT_ACCENT: &str = "#5b8dd6";

/// 认得的深浅色。其余值一律当「跟随系统」
const THEMES: [&str; 3] = ["system", "light", "dark"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    /// 界面缩放（1.0 = 100%）
    pub zoom: f64,
    /// 主题色（`#rrggbb`）
    pub accent: String,
    /// `system` / `light` / `dark`
    pub theme: String,
    /// 渲染区是否限宽
    pub limit_width: bool,
    /// 标签栏**开局**是否收起。
    ///
    /// 只是默认值：这一次会话里在标签栏上展开/收起不写盘 —— 那是高频操作，
    /// 每点一下写一次文件不值当。
    pub rail_collapsed: bool,
    /// 代码块是否显示行号
    pub code_line_numbers: bool,
    /// 记不记浏览历史（记下来的在 `settings/browsing.json`，随时可以单独清空）
    pub record_history: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            accent: DEFAULT_ACCENT.to_string(),
            theme: "system".to_string(),
            limit_width: true,
            rail_collapsed: false,
            code_line_numbers: true,
            record_history: true,
        }
    }
}

impl Preferences {
    /// 把值收进合法范围。
    ///
    /// 文件是用户看得见、手改得了的，前端送来的值也不值得全信；与其让界面带着一个
    /// 没法用的值跑起来，不如在这里悄悄纠正。读与写都过这一道，于是落盘的一定合法。
    fn sanitized(mut self) -> Self {
        self.zoom = if self.zoom.is_finite() {
            self.zoom.clamp(ZOOM_MIN, ZOOM_MAX)
        } else {
            1.0
        };

        let accent = self.accent.trim().to_lowercase();
        self.accent = if is_hex_color(&accent) {
            accent
        } else {
            DEFAULT_ACCENT.to_string()
        };

        if !THEMES.contains(&self.theme.as_str()) {
            self.theme = "system".to_string();
        }

        self
    }
}

/// 读偏好；**文件不在就先落一份默认的**。
///
/// 该在的东西第一次打开就该在：不然"改过一次设置"之前，偏好文件一直不存在，
/// 让人以为它没被用上。
pub fn load(workspace: &Workspace) -> Result<Preferences, String> {
    let path = workspace.settings_file(PREFERENCES_FILE);

    if !path.exists() {
        let defaults = Preferences::default();
        write_json(&path, &defaults)?;
        return Ok(defaults);
    }

    // 手改过的文件也要收进合法范围
    Ok(read_json::<Preferences>(&path).sanitized())
}

/// 写入偏好，返回**实际存下去的那一份**（可能已被收进合法范围）
pub fn save(workspace: &Workspace, preferences: Preferences) -> Result<Preferences, String> {
    let preferences = preferences.sanitized();
    write_json(&workspace.settings_file(PREFERENCES_FILE), &preferences)?;
    Ok(preferences)
}

/// `#rrggbb`：井号加六位十六进制
fn is_hex_color(text: &str) -> bool {
    let Some(digits) = text.strip_prefix('#') else {
        return false;
    };
    digits.len() == 6 && digits.chars().all(|ch| ch.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn scratch(name: &str) -> Workspace {
        let dir = std::env::temp_dir().join(format!(
            "refind-note-preferences-test-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        Workspace::open(dir).unwrap()
    }

    fn cleanup(workspace: &Workspace) {
        let _ = fs::remove_dir_all(workspace.root());
    }

    #[test]
    fn defaults_are_the_first_run_values() {
        let workspace = scratch("defaults");

        assert_eq!(
            load(&workspace).unwrap(),
            Preferences {
                zoom: 1.0,
                accent: DEFAULT_ACCENT.to_string(),
                theme: "system".to_string(),
                limit_width: true,
                rail_collapsed: false,
                code_line_numbers: true,
                ..Default::default()
            }
        );

        cleanup(&workspace);
    }

    #[test]
    fn everything_survives_one_round_trip() {
        let workspace = scratch("roundtrip");

        let saved = save(
            &workspace,
            Preferences {
                zoom: 1.25,
                accent: "#3F9E9E".to_string(),
                theme: "dark".to_string(),
                limit_width: false,
                rail_collapsed: true,
                code_line_numbers: false,
                ..Default::default()
            },
        )
        .unwrap();

        // 主题色顺便规范成小写
        assert_eq!(saved.accent, "#3f9e9e");
        assert_eq!(load(&workspace).unwrap(), saved);

        cleanup(&workspace);
    }

    #[test]
    fn the_file_is_created_on_the_first_open() {
        let workspace = scratch("created");
        let path = workspace.settings_file(PREFERENCES_FILE);

        assert!(!path.exists(), "前提：这份工作目录是空的");
        let preferences = load(&workspace).unwrap();

        // 文件该在的东西第一次打开就该在，不必等改过设置
        assert!(path.is_file(), "打开一次就该把默认偏好落下来");
        assert_eq!(read_json::<Preferences>(&path), preferences);

        cleanup(&workspace);
    }

    #[test]
    fn out_of_range_values_are_pulled_back() {
        let workspace = scratch("clamped");

        let saved = save(
            &workspace,
            Preferences {
                zoom: 99.0,
                accent: "不是颜色".to_string(),
                theme: "rainbow".to_string(),
                limit_width: false,
                rail_collapsed: false,
                code_line_numbers: true,
                ..Default::default()
            },
        )
        .unwrap();

        assert_eq!(saved.zoom, ZOOM_MAX);
        assert_eq!(saved.accent, DEFAULT_ACCENT);
        assert_eq!(saved.theme, "system");
        // 限宽是布尔，没有"不合法"可言，原样留着
        assert!(!saved.limit_width);

        cleanup(&workspace);
    }

    #[test]
    fn a_hand_edited_file_is_corrected_on_load() {
        let workspace = scratch("hand-edited");
        let path = workspace.settings_file(PREFERENCES_FILE);

        fs::write(
            &path,
            r##"{"zoom":0.01,"accent":"#ABCDEF  ","theme":"LIGHT"}"##,
        )
        .unwrap();

        let preferences = load(&workspace).unwrap();
        assert_eq!(preferences.zoom, ZOOM_MIN);
        assert_eq!(preferences.accent, "#abcdef", "首尾空白去掉、字母收成小写");
        assert_eq!(preferences.theme, "system", "大小写不匹配也算不认得");
        // 没写的那一项用默认值，而不是整份读失败
        assert!(preferences.limit_width);
        assert!(!preferences.rail_collapsed);

        cleanup(&workspace);
    }

    #[test]
    fn a_broken_file_reads_as_defaults_instead_of_failing() {
        let workspace = scratch("broken");
        let path = workspace.settings_file(PREFERENCES_FILE);

        fs::write(&path, "{ 这不是 JSON").unwrap();
        assert_eq!(load(&workspace).unwrap(), Preferences::default());

        cleanup(&workspace);
    }
}
