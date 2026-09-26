//! 数据库模型版本。
//!
//! 语义由项目约定，写在这里免得走样：
//!
//! - **大版本**变化是严重破坏性更改，**只能从头重建** —— 程序直接拒绝打开；
//! - **中版本**是功能增加，**可升级** —— 打开时接受，并把仓库里的版本改成程序这一版；
//! - **小版本**是 bug 修复 —— 同样接受并记录。
//!
//! 还有一种情况要单独挡住：**仓库比程序新**（用户降级了程序）。数据里可能已经有这一版
//! 不认识的写法，硬着头皮打开只会写坏它，所以同样拒绝 —— 但提示词不一样，
//! 让人一眼看出该升程序还是该重建仓库。
use std::cmp::Ordering;

/// 当前程序认的模型版本。改它的时候请同时想清楚：这是大、中、小哪一种变化。
pub const MODEL_VERSION: &str = "1.0.0";

/// 解析出来的三段版本号
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelVersion {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl ModelVersion {
    /// 解析 `大.中.小`。只认纯数字，额外后缀（`-beta`）不接受 ——
    /// 模型版本是要拿来判定兼容性的，不是给人读的花哨字符串。
    pub fn parse(text: &str) -> Option<Self> {
        let mut parts = text.trim().split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next()?.parse().ok()?;
        let patch = parts.next()?.parse().ok()?;
        if parts.next().is_some() {
            return None;
        }
        Some(Self {
            major,
            minor,
            patch,
        })
    }
}

impl std::fmt::Display for ModelVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// 仓库里的版本与程序这一版之间的关系
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Compatibility {
    /// 完全一致，什么都不用做
    Same,
    /// 仓库更旧、但可以就地升级：把仓库里的版本号写成程序这一版
    Upgrade { from: ModelVersion },
}

/// 判断仓库里的模型版本能不能用。
///
/// 返回 `Err(一句话)` 时，那句话是**直接给用户看的**：说清发生了什么、该怎么办。
pub fn check(stored_text: &str) -> Result<Compatibility, String> {
    let Some(stored) = ModelVersion::parse(stored_text) else {
        return Err(format!(
            "仓库里的模型版本读不出来（写的是「{stored_text}」）。\
             本程序认的格式是「大.中.小」，例如 {MODEL_VERSION}。\
             可以手工把 vault.json 里的 model_version 改对，或从头重建仓库。"
        ));
    };
    decide(stored, ours()?)
}

/// 程序这一版（常量写错时给出人话的报错）
fn ours() -> Result<ModelVersion, String> {
    ModelVersion::parse(MODEL_VERSION)
        .ok_or_else(|| format!("程序内置的模型版本常量写错了：{MODEL_VERSION}"))
}

/// 判定本身。拆出来是为了**能测**：程序当前是 1.0.0，"更旧的同大版本"这种输入
/// 从真实版本号里造不出来（1.0.0 就是最小的 1.x），但升级这条路必须被验证过。
fn decide(stored: ModelVersion, ours: ModelVersion) -> Result<Compatibility, String> {
    if stored.major != ours.major {
        return Err(format!(
            "仓库的模型版本是 {stored}，本程序只认 {ours}。\
             大版本变化是破坏性的，两边不能混用 —— 这个仓库需要从头重建。"
        ));
    }

    match stored.cmp(&ours) {
        Ordering::Equal => Ok(Compatibility::Same),
        // 仓库更旧：功能增加或修 bug，都可以就地升级
        Ordering::Less => Ok(Compatibility::Upgrade { from: stored }),
        // 仓库更新：用户降级了程序。硬开只会写坏数据
        Ordering::Greater => Err(format!(
            "仓库的模型版本是 {stored}，比本程序的 {ours} 新。\
             请升级程序；若确实要用旧程序，只能从头重建仓库。"
        )),
    }
}

impl PartialOrd for ModelVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ModelVersion {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.major, self.minor, self.patch).cmp(&(other.major, other.minor, other.patch))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_three_segments_only() {
        assert_eq!(ModelVersion::parse("1.0.0").unwrap().major, 1);
        assert_eq!(ModelVersion::parse(" 2.3.4 ").unwrap().minor, 3);
        assert!(ModelVersion::parse("1.0").is_none());
        assert!(ModelVersion::parse("1.0.0.0").is_none());
        assert!(ModelVersion::parse("1.0.0-beta").is_none());
        assert!(ModelVersion::parse("").is_none());
        assert!(ModelVersion::parse("一.二.三").is_none());
    }

    #[test]
    fn the_builtin_version_parses() {
        // 常量写错是最容易发生、又最难发现的事故：让它在这里失败
        assert!(ModelVersion::parse(MODEL_VERSION).is_some());
    }

    #[test]
    fn same_version_needs_nothing() {
        assert_eq!(check(MODEL_VERSION), Ok(Compatibility::Same));
    }

    #[test]
    fn older_minor_and_patch_are_upgradeable() {
        // 程序 1.2.3 遇到 1.2.0 / 1.0.9：都是同大版本，就地升级
        let ours = ModelVersion { major: 1, minor: 2, patch: 3 };
        for text in ["1.2.0", "1.0.9", "1.2.2"] {
            let stored = ModelVersion::parse(text).unwrap();
            match decide(stored, ours) {
                Ok(Compatibility::Upgrade { from }) => assert_eq!(from.to_string(), text),
                other => panic!("{text} 应当可升级，实际 {other:?}"),
            }
        }
    }

    #[test]
    fn another_major_is_refused_and_says_rebuild() {
        let ours = ModelVersion { major: 1, minor: 0, patch: 0 };
        for text in ["2.0.0", "0.9.9"] {
            let stored = ModelVersion::parse(text).unwrap();
            let message = decide(stored, ours).unwrap_err();
            assert!(message.contains("从头重建"), "{message}");
        }
    }

    #[test]
    fn newer_version_is_refused_without_saying_rebuild() {
        let ours = ModelVersion { major: 1, minor: 0, patch: 0 };
        let message = decide(ModelVersion::parse("1.9.9").unwrap(), ours).unwrap_err();
        assert!(message.contains("请升级程序"), "{message}");
        // 重建只是"非要降级"时的退路：它出现在升级建议**之后**，不该是主建议
        let upgrade = message.find("请升级程序").expect("应当建议升级");
        let rebuild = message.find("从头重建").expect("应当给出退路");
        assert!(upgrade < rebuild, "主建议应当是升级程序：{message}");
    }

    #[test]
    fn garbage_is_refused_with_a_readable_message() {
        let message = check("一.二.三").unwrap_err();
        assert!(message.contains("读不出来"), "{message}");
        assert!(message.contains("model_version"), "要指出改哪里：{message}");
    }
}
