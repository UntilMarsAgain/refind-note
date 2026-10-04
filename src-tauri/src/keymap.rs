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

//! 键位：工作目录 `settings/keymap.json`。
//!
//! ## 为什么单独一个文件，不进 `preferences.json`
//!
//! 偏好是「这台机器上用着舒不舒服」的一堆小开关（缩放、主题色、要不要行号），
//! 手改的时候一眼能扫完。键位是另一回事：
//!
//! - **形状不同**。偏好是一组标量字段，键位是「动作 → 键」的一张**表**，而且
//!   往后只会越来越长。
//! - **改动频率不同**。偏好很少动；键位是会被反复改的东西（按着不顺手就换），
//!   得方便手改和排查"我到底把哪个键设成了什么"。
//! - **坏了的症状不同**。偏好读不动退回默认，只是颜色不对；键位读不动退回默认，
//!   是所有快捷键都失效 —— 所以它必须能独立查、独立删。
//!
//! 放一起会把那个文件撑成一个两种东西混排的清单，改键还得在偏好中间找位置。
//!
//! ## 存什么
//!
//! **只存"与默认值不同"的项**，其余走 [`DEFAULTS`]。这样文件平时几乎是空的，
//! 手改时看得懂；将来新增动作也不会把老用户的文件撑出一堆没写的项。
//!
//! 键位写成 `["Ctrl", "Shift", "T"]` 这样的**键序列**（修饰键在前、字母在后），
//! 而不是 `"Ctrl+Shift+T"` 那种字符串：捕捉按键时按下的就是一串独立的键，
//! 存成同一形状，识别与显示才不用反复去拆字符串。

use serde::{Deserialize, Serialize};

use crate::storage::workspace::{read_json, write_json, Workspace};

const KEYMAP_FILE: &str = "keymap.json";

/// 认识的动作，以及它们**出厂**的键。
///
/// 这份表是"这个程序有哪些能改键的动作"的唯一出处：设置页照它列行、菜单照它显示
/// 右边那排小方键、`sanitized()` 照它剔掉不认识的动作。新增一个动作只改这一处。
///
/// 键位写成 `Ctrl`（macOS 上界面显示 `Cmd`，但**存的一律是 `Ctrl`** —— 存的是
/// `event.ctrlKey` / `event.metaKey` 这边的事实，显示成什么由前端决定）。
pub const DEFAULTS: &[(&str, &[&str])] = &[
    // 标签页
    ("new-tab", &["Ctrl", "T"]),
    ("reopen-closed", &["Ctrl", "Shift", "T"]),
    ("close-tab", &["Ctrl", "W"]),
    // 查找
    ("find", &["Ctrl", "F"]),
    ("find-next", &["F3"]),
    ("find-previous", &["Shift", "F3"]),
    // 页面
    // 方向键是 `ArrowLeft` / `ArrowRight` —— 前端 `core/keymap.ts` 的 `ACTIONS`
    // 里必须一字不差地写同样这两个（那边认的是 `event.key`，规范里的写法就是它；
    // `"Left"` 是已废弃的旧别名，键位会永远匹配不上）。
    ("back", &["Alt", "ArrowLeft"]),
    ("forward", &["Alt", "ArrowRight"]),
    ("home", &["Alt", "H"]),
    ("menu", &["Alt", "M"]),
    ("reload", &["Ctrl", "R"]),
];

/// 认得的修饰键。存进来之前一律归一到这几个写法。
const MODIFIERS: [&str; 6] = ["Ctrl", "Shift", "Alt", "Meta", "CapsLock", "Fn"];

/// 键位表：动作 id → 键序列。
///
/// 是一张**稀疏**的表：这里没有的键位表示"用出厂的"，见 [`DEFAULTS`]。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Keymap {
    /// 每个动作一条覆盖；动作 id → 键序列
    #[serde(flatten)]
    pub bindings: std::collections::BTreeMap<String, Vec<String>>,
}

/// `Default` 是**出厂的那一份**，不是一张空表。
///
/// 这一条不是随手定的：`storage::workspace::read_json` 读不动文件时给的就是
/// `Default::default()`，所以"空的"默认值会让**坏掉的 json 关掉全部快捷键** ——
/// 而那正是这个模块开头说"最糟"的那种失效。空表读进来，`keys_of` 什么也答不出来。
impl Default for Keymap {
    fn default() -> Self {
        Self::defaults()
    }
}

impl Keymap {
    /// 出厂的那一份
    pub fn defaults() -> Self {
        let mut bindings = std::collections::BTreeMap::new();
        for (action, keys) in DEFAULTS {
            bindings.insert(
                (*action).to_string(),
                keys.iter().map(|k| (*k).to_string()).collect(),
            );
        }
        Self { bindings }
    }

    /// 某动作**实际生效**的键：用户改了就用用户的，没改用出厂的。
    pub fn keys_of(&self, action: &str) -> Vec<String> {
        if let Some(keys) = self.bindings.get(action) {
            return keys.clone();
        }
        DEFAULTS
            .iter()
            .find(|(name, _)| *name == action)
            .map(|(_, keys)| keys.iter().map(|k| (*k).to_string()).collect())
            .unwrap_or_default()
    }

    /// 某动作**用户有没有改过**（界面据此显示"恢复默认"是不是可点的）
    pub fn is_customized(&self, action: &str) -> bool {
        self.bindings.contains_key(action)
    }

    /// 收进合法范围：认识的键位写法、不认识的动作、以及修饰键顺序。
    ///
    /// 用户看得见也可能手改这个文件，所以与 [`crate::settings`] 的 `sanitized()`
    /// 同一个原则：**不让程序起不来**。看不懂的一律退回默认，而不是报错。
    pub fn sanitized(mut self) -> Self {
        // 认识的键位写法统一成一份对照表（`ctrl` / `control` / `CTRL` 都算 Ctrl）
        let mut cleaned = std::collections::BTreeMap::new();

        for (action, keys) in std::mem::take(&mut self.bindings) {
            if !DEFAULTS.iter().any(|(name, _)| *name == action) {
                continue;
            }
            let binding = sanitize_keys(&keys);
            // 什么都不剩（写的是 `[""]`、`[]`）等于没写这条 → 落回出厂，
            // 而不是留下一个"什么都触发不了"的空键位。
            if binding.is_empty() {
                continue;
            }
            cleaned.insert(action, binding);
        }

        Self { bindings: cleaned }
    }
}

/// 一条键位收进合法写法：修饰键在前（按固定次序）、后面至多一个普通键
fn sanitize_keys(keys: &[String]) -> Vec<String> {
    let mut modifiers: Vec<String> = Vec::new();
    let mut plain: Option<String> = None;

    for key in keys {
        let normalized = match key.trim().to_lowercase().as_str() {
            "ctrl" | "control" | "ctl" => "Ctrl",
            "shift" => "Shift",
            "alt" | "option" => "Alt",
            "meta" | "cmd" | "command" | "super" => "Meta",
            "capslock" | "caps" => "CapsLock",
            "fn" => "Fn",
            _ => "",
        };

        if !normalized.is_empty() {
            // 同一个修饰键写两遍不算错，去了重（`["Ctrl","Ctrl","F"]` → `["Ctrl","F"]`）
            let name = normalized.to_string();
            if !modifiers.contains(&name) {
                modifiers.push(name);
            }
            continue;
        }

        let trimmed = key.trim();
        if trimmed.is_empty() {
            continue;
        }
        // 两个普通键按下去也不该触发（那是"敲一串"，不是"按一个快捷键"）——
        // 保留第一个，多出来的丢掉
        if plain.is_none() {
            plain = Some(trimmed.to_string());
        }
    }

    // 修饰键按固定次序排（存盘后的文件看起来才是一套的）
    modifiers.sort_by_key(|name| {
        MODIFIERS
            .iter()
            .position(|m| m == name)
            .unwrap_or(usize::MAX)
    });

    if let Some(key) = plain {
        modifiers.push(key);
    }
    modifiers
}

/// 读键位；**文件不在就先落一份出厂的**。
pub fn load(workspace: &Workspace) -> Result<Keymap, String> {
    let path = workspace.settings_file(KEYMAP_FILE);

    if !path.exists() {
        let defaults = Keymap::defaults();
        write_json(&path, &defaults)?;
        return Ok(defaults);
    }

    // 手改过的文件也要收进合法范围
    Ok(read_json::<Keymap>(&path).sanitized())
}

/// 写键位，返回**实际存下去的那一份**（可能已被收进合法范围）
pub fn save(workspace: &Workspace, keymap: Keymap) -> Result<Keymap, String> {
    let keymap = keymap.sanitized();
    write_json(&workspace.settings_file(KEYMAP_FILE), &keymap)?;
    Ok(keymap)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn scratch(name: &str) -> Workspace {
        let dir = std::env::temp_dir().join(format!(
            "refind-note-keymap-test-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        Workspace::open(dir).unwrap()
    }

    fn cleanup(workspace: &Workspace) {
        let _ = fs::remove_dir_all(workspace.root());
    }

    /// 从一段 JSON 文本读键位（模拟手改过的文件）
    fn from_json(text: &str) -> Keymap {
        serde_json::from_str::<Keymap>(text).unwrap().sanitized()
    }

    #[test]
    fn the_defaults_are_the_first_run_values() {
        let workspace = scratch("defaults");

        assert_eq!(load(&workspace).unwrap(), Keymap::defaults());
        // 该在的东西第一次打开就该在：与 `settings.rs` 同一个理由
        assert!(workspace.settings_file(KEYMAP_FILE).exists());

        cleanup(&workspace);
    }

    #[test]
    fn a_customized_binding_wins_over_the_default() {
        let workspace = scratch("override");

        let map = Keymap {
            bindings: [("find".to_string(), vec!["Alt".to_string(), "K".to_string()])]
                .into_iter()
                .collect(),
        };
        let stored = save(&workspace, map).unwrap();

        assert_eq!(stored.keys_of("find"), vec!["Alt", "K"]);
        // 没写的那几个仍走出厂的
        assert_eq!(stored.keys_of("new-tab"), vec!["Ctrl", "T"]);
        assert!(stored.is_customized("find"));
        assert!(!stored.is_customized("new-tab"));

        // 重开一次仍是这一份
        assert_eq!(load(&workspace).unwrap(), stored);

        cleanup(&workspace);
    }

    #[test]
    fn the_file_is_flat_so_a_person_can_edit_it() {
        let workspace = scratch("flat");

        let map = Keymap {
            bindings: [
                (
                    "find".to_string(),
                    vec!["Ctrl".to_string(), "F".to_string()],
                ),
                (
                    "new-tab".to_string(),
                    vec!["Ctrl".to_string(), "T".to_string()],
                ),
            ]
            .into_iter()
            .collect(),
        };
        save(&workspace, map).unwrap();

        let text = fs::read_to_string(workspace.settings_file(KEYMAP_FILE)).unwrap();
        // **摊平**一层（`#[serde(flatten)]`）：手改时一眼看得出是什么。
        // 前端 `src/ipc/keymap.ts` 的形状就是照这个写的 —— 嵌一层那边就得跟着改。
        // （别去断言 `"find": ["Ctrl", "F"]` 那种写法：`to_vec_pretty` 会把数组
        //   的每个元素各占一行，这里只关心结构，不关心换行。）
        assert!(text.contains(r#""find""#), "{text}");
        assert!(!text.contains("bindings"), "不该嵌一层：\n{text}");
        // 顶层的键就是动作 id
        let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
        let object = parsed.as_object().expect("顶层该是对象");
        assert!(
            object.contains_key("find") && object.contains_key("new-tab"),
            "{text}"
        );
        assert_eq!(object["find"][0], "Ctrl");

        cleanup(&workspace);
    }

    #[test]
    fn modifier_spellings_are_folded_to_one() {
        let map = from_json(r#"{"find": ["control", "SHIFT", "f"]}"#);
        assert_eq!(map.keys_of("find"), vec!["Ctrl", "Shift", "f"]);
    }

    #[test]
    fn modifiers_come_out_in_a_fixed_order() {
        // 顺序由程序定，不由用户写的时候的顺序定：这样存盘的文件看起来是一套的
        let map = from_json(r#"{"find": ["T", "Alt", "Shift", "Ctrl"]}"#);
        assert_eq!(map.keys_of("find"), vec!["Ctrl", "Shift", "Alt", "T"]);
    }

    #[test]
    fn unknown_actions_and_junk_bindings_are_dropped() {
        let map = from_json(
            r#"{"find": ["Ctrl","F"], "no-such-action": ["Ctrl","Q"], "close-tab": [""]}"#,
        );

        // 不认识的动作：丢（它是哪来的只能是人手写错的）
        assert!(!map.bindings.contains_key("no-such-action"));
        // 空键位也丢：留着等于"什么都触发不了"，反而不如走出厂的
        assert!(!map.keys_of("close-tab").is_empty(), "空键位该落回出厂的");
        // 合法的照旧
        assert_eq!(map.keys_of("find"), vec!["Ctrl", "F"]);
    }

    #[test]
    fn more_than_one_plain_key_collapses_to_the_first() {
        // `["Ctrl","A","B"]` 是"敲一串"，不是"按一个快捷键"
        let map = from_json(r#"{"find": ["Ctrl","A","B"]}"#);
        assert_eq!(map.keys_of("find"), vec!["Ctrl", "A"]);
    }

    #[test]
    fn a_repeated_modifier_is_collapsed() {
        let map = from_json(r#"{"find": ["Ctrl","Ctrl","F"]}"#);
        assert_eq!(map.keys_of("find"), vec!["Ctrl", "F"]);
    }

    #[test]
    fn a_broken_file_reads_as_defaults_instead_of_failing() {
        let workspace = scratch("broken");

        // 半截 JSON —— 程序起不来是最糟的结果，读不动就该当没设过
        fs::write(workspace.settings_file(KEYMAP_FILE), "{ not json").unwrap();

        assert_eq!(load(&workspace).unwrap(), Keymap::defaults());

        cleanup(&workspace);
    }

    #[test]
    fn no_default_binding_uses_a_deprecated_key_name() {
        // `KeyboardEvent.key` 对方向键给的是 `ArrowLeft`/`ArrowRight`。
        // 写成 `"Left"`（老代码里那个已废弃的别名）的话，键位**永远匹配不上** ——
        // 而显示那侧因为不认识它会把 `←` 原样打出来，看着倒像是对的，
        // 于是"提示里写着 Alt+←、按了没反应"这种问题极难发现。
        //
        // 前端 `core/keymap.ts` 的 `ACTIONS` 里必须写同样这几个字，两边对不上
        // 就是按不出来，所以这里挡一次。
        let deprecated = ["Left", "Right", "Up", "Down", "Esc", "Spacebar", "Del"];
        for (action, keys) in DEFAULTS {
            for key in *keys {
                assert!(
                    !deprecated.contains(key),
                    "「{action}」用了已废弃的键名 {key}（应为 Arrow{key} 之类）"
                );
            }
        }
    }

    #[test]
    fn every_default_action_is_unique() {
        // 两个动作抢同一个键：先注册的赢，另一个按不出来 ——
        // 菜单上写着按不出来比不写更糟，所以这一条得挡住
        let mut seen = std::collections::HashMap::new();
        for (action, keys) in DEFAULTS {
            let binding = keys.join("+");
            if let Some(previous) = seen.insert(binding.clone(), *action) {
                panic!("「{previous}」与「{action}」抢同一个键 {binding}");
            }
        }
    }
}
