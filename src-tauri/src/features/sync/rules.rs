//! **判定**：一个路径该怎么办。
//!
//! 同步真正的难处不在搬文件，而在**决定搬不搬、删不删**。这一层因此被剥成一个纯函数
//! [`decide`]：给它"本机有没有 / 云端有没有 / 上次对齐是什么样 / 有没有删除凭据"，它还回
//! 一个 [`Decision`] 与一句人话。它不碰网络、不碰磁盘 —— 同步最怕的就是"某种情形下悄悄
//! 丢了东西"，而那种情形只能靠把分支一个个列出来盯着（见 `tests`）。
//!
//! ## 判定的语义（三句话）
//!
//! 1. **变没变**：本机看内容指纹（`db/blobs/**` 名字就是指纹），云端看 ETag ——
//!    索引（`settings/sync-index.json`）记着上次对齐时的样子，两头都比它。
//! 2. **谁新谁旧**：只有两边都动过时才需要取舍，按修改时间；时间一样就**偏保守**留本机。
//! 3. **删**：删除要**两处都认、而且有凭据**才动（`Evidence`）。清单里没有某一份
//!    **不等于**云端删过它 —— 这条是从真事事故来的，见 `CloudSays`。

use super::settings::Stamp;

/// 本机这一份
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Local {
    /// 内容指纹：**内容寻址**的那些直接用文件名（名字就是哈希），其余算一遍 sha256。
    ///
    /// 为什么不用"大小 + 修改时间"那种便宜的判法：同一秒里改一笔、正好还一样长，
    /// 那种判法看不出来（测试里就逮到过一次）。这一份仓库不大，算一遍更踏实。
    pub(super) hash: String,
    pub(super) mtime: i64,
}

/// 云端这一份
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Remote {
    pub(super) size: u64,
    pub(super) etag: String,
    /// 最后修改时间（Unix 秒）
    pub(super) modified: i64,
}

/// 云端那份账对某一条怎么说（判断"云端删没删"的证据）
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum CloudSays {
    /// 云端压根没有这份账（第一次用、换了桶、账本丢了）—— **没有证据**
    NoAccount,
    /// 账上还记着它 —— 清单里没有只是清单的问题
    Listed,
    /// 账上也没有它 —— 两处一致，确实是被删了
    Gone,
}

/// 判"删没删"要的两处证据
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Evidence {
    pub(super) cloud: CloudSays,
    /// 本机这边**留着删除凭据**吗（笔记的日志进了回收站）
    pub(super) trashed: bool,
}

impl Evidence {
    /// 什么凭据都没有时的那一份
    ///
    /// 只有测试用得上（引擎那侧总是显式说出两处证据），所以只在测试里存在。
    #[cfg(test)]
    pub(super) fn none() -> Self {
        Self {
            cloud: CloudSays::NoAccount,
            trashed: false,
        }
    }
}

/// 一个路径该怎么办
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Decision {
    /// 两边一样，只把索引擦新
    Nothing,
    Upload,
    Download,
    /// 本机删过 → 云端也删
    DeleteRemote,
    /// 云端删过 → 本机也删
    DeleteLocal,
    /// 两边都动过：按时间取新的那一边
    TakeNewer(&'static str),
}

/// 规则都在这里：给定"本机有没有、云端有没有、上次对齐是什么样"，决定怎么办。
///
/// 单独写成一个纯函数，是为了能**不碰网络**把每条分支都试一遍 —— 同步最怕的就是
/// "某种情形下悄悄丢了东西"，而那只能靠把这些情形一个个列出来盯着。
pub(super) fn decide(
    force_upload: bool,
    local: Option<&Local>,
    remote: Option<&Remote>,
    aligned: Option<&Stamp>,
    evidence: Evidence,
) -> (Decision, String) {
    // 换过钥匙的那一趟：本机有的一律重传（云端那些旧密文已经解不开了）
    if force_upload {
        return match (local, remote) {
            (Some(_), _) => (Decision::Upload, "重传这一份".to_string()),
            (None, Some(_)) => (Decision::Download, String::new()),
            (None, None) => (Decision::Nothing, String::new()),
        };
    }

    match (local, remote, aligned) {
        // ---- 两边都有 ----
        (Some(local), Some(remote), aligned) => {
            // "变没变"看两样：本机看（大小、时间），云端看 ETag
            let local_changed = aligned
                .map(|stamp| stamp.hash != local.hash)
                .unwrap_or(true);
            // 比 ETag 不认大小写：它是十六进制，真变了不会只差大小写；而不同的接口
            // （清单 / GET / HEAD）偶尔就是这么点差别 —— 认成"变了"就会每趟重下一遍
            let remote_changed = aligned
                .map(|stamp| !stamp.etag.eq_ignore_ascii_case(&remote.etag))
                .unwrap_or(true);

            match (local_changed, remote_changed) {
                (false, false) => (Decision::Nothing, String::new()),
                (true, false) => (Decision::Upload, "本机这份变了".to_string()),
                (false, true) => (Decision::Download, "云端这份变了".to_string()),
                (true, true) => {
                    // 两边都动过：内容块不会走到这里（内容寻址），
                    // 剩下的用时间定；时间也一样时**偏保守**，留本机的那份
                    if remote.modified > local.mtime + 1 {
                        (Decision::TakeNewer("remote"), "云端那份更新".to_string())
                    } else {
                        (Decision::TakeNewer("local"), "本机这份不旧".to_string())
                    }
                }
            }
        }

        // ---- 只有本机有 ----
        (Some(local), None, aligned) => match aligned {
            // 上次对齐时云端有它、现在清单里没了 —— **先别急着删**。
            //
            // "清单里没有"有三种可能：别的机器删了、清单没列全、桶被清空/换了。
            // 只有**云端那份账也记着它没了**，才是第一种。分不清就别删 ——
            // "本地新写的东西被当成旧版本删掉"就是这么来的。
            Some(stamp)
                if stamp.hash == local.hash && evidence.cloud == CloudSays::Gone =>
            {
                (Decision::DeleteLocal, "云端已经删掉".to_string())
            }
            Some(stamp) if stamp.hash == local.hash => (
                Decision::Upload,
                "云端清单里没有它，账上还在 —— 当作没传上去".to_string(),
            ),
            Some(_) => (Decision::Upload, "云端删过，但本机这份改过".to_string()),
            // 从来没有过：新写的，传上去
            None => (Decision::Upload, String::new()),
        },

        // ---- 只有云端有 ----
        (None, Some(_), aligned) => match aligned {
            // 本机删过 —— 但**"本机没有"和"我删的"是两回事**：删除会留下凭据
            // （笔记的日志进回收站，见 `trashed_at`）。有凭据才替人删云端那份；
            // 没有就取回来 —— 宁可多一份，也别把云端唯一的那一份抹掉。
            Some(_) if evidence.trashed => {
                (Decision::DeleteRemote, "本机已经删掉".to_string())
            }
            Some(_) => (
                Decision::Download,
                "云端还有这一份、本机没了 —— 取回来（要删它请在本机删）".to_string(),
            ),
            None => (Decision::Download, String::new()),
        },

        // ---- 两边都没有 ----
        (None, None, _) => match aligned {
            // 索引里有、两边都没了：擦掉这条记录
            Some(_) => (Decision::Nothing, String::new()),
            None => (Decision::Nothing, String::new()),
        },
    }
}