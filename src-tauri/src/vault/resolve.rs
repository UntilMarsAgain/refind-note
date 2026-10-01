//! 地址的**语义解析**：把语法解析的结果落到仓库上。
//!
//! 语法（前缀、状态词、章节）在 `address` 里；这里回答的是"这一页在不在、是哪种地方"：
//! 特殊页、还不存在的页、随机跳转，以及把版本 token 解释成版本号。
//! 内容本身（含上锁状态）由 [`Database::read_note`] 走 `notes` 那套。

use serde::Serialize;

use crate::storage::codec::{self, EncryptionReport, Policy, Secrets, SignatureReport};
use crate::storage::session;
use crate::vault::address::{self, Address, Mode, ParsedAddress};
use crate::vault::database::Database;
use crate::vault::namespace::SPECIAL_ID;
use crate::vault::notes::Reading;
use crate::vault::title::ParsedTitle;

/// 地址落到仓库上的结论："这是什么地方"
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Outcome {
    /// 仓库里有这一页
    Note { title: String },
    /// 仓库里还没有这一页（交给"创建"那条路）
    Missing { title: String },
    /// 特殊页面（虚拟命名空间）：前端按 `page` 选视图
    Special { page: String },
}

/// 地址 + 它落到仓库上的结论
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ResolvedAddress {
    pub address: Address,
    pub canonical: String,
    pub outcome: Outcome,
}

impl Database {
    /// 解析地址栏那一行并落到仓库上；空输入不是地址（`None`）。
    ///
    /// - 特殊页：`special:random` 是**一次跳转**，挑一篇笔记落下去（返回目标那篇的结果）；
    /// - 还不存在的页：状态裁掉（`名称@edit` 与 `名称` 回显成同一个地址）——
    ///   状态只在"这一页存在"的那一支上有意义。
    pub fn resolve_address(&self, input: &str) -> Result<Option<ResolvedAddress>, String> {
        let table = self.namespaces();
        let Some(parsed) = address::parse(input, &table)? else {
            return Ok(None);
        };
        let ParsedAddress { address, canonical } = parsed;

        if address.namespace.id == SPECIAL_ID {
            if address.page.eq_ignore_ascii_case("random") {
                return self.resolve_random();
            }
            let page = address.page.to_lowercase();
            return Ok(Some(ResolvedAddress {
                address,
                canonical,
                outcome: Outcome::Special { page },
            }));
        }

        // 给出去的是**显示标题**（命名空间写成规范名）：前端拿它拼地址、让后端读，
        // 与人在地址栏里敲 `帮助:入门` 是同一件事 —— 别名在这里收敛成规范名
        let title = ParsedTitle {
            ns: address.namespace.id.clone(),
            page: address.page.clone(),
        }
        .display(&table);

        if self.exists(&title) {
            return Ok(Some(ResolvedAddress {
                address,
                canonical,
                outcome: Outcome::Note { title },
            }));
        }

        // 还不存在的页：把状态裁掉再看
        let cropped = Address {
            mode: Mode::View { reference: None },
            ..address
        };
        let canonical = address::compose(&cropped);
        Ok(Some(ResolvedAddress {
            address: cropped,
            canonical,
            outcome: Outcome::Missing { title },
        }))
    }

    /// `special:random`：挑一篇笔记，落到它身上
    fn resolve_random(&self) -> Result<Option<ResolvedAddress>, String> {
        let titles: Vec<String> = self.titles()?.notes.values().cloned().collect();
        let Some(title) = pick_one(&titles)? else {
            return Err("仓库里还没有笔记，随机跳转没地方去".to_string());
        };
        self.resolve_address(title)
    }

    /// 读某一版：`reference` 是地址里的 token，`None` = 最新版
    pub fn read_note(&self, title: &str, reference: Option<&str>) -> Result<Reading, String> {
        match reference {
            None => self.read_for_display(title),
            Some(token) => self.read_revision_for_display(title, token_to_rev(token)?),
        }
    }

    /// 回滚到某一版（`reference` 是地址里的版本 token；`copy` 见 [`Self::rollback_copy`]）。
    ///
    /// `protection` 与 `passphrase` 只对"重写"这一支有意义：显式给出保护就是**换保护**
    /// （从新这一版起粘住），不给就照这篇当前的保护；口令只在这一版要套对称层时用得上。
    /// 复制那一支整个封装都跟着旧版，两者都不受影响。
    /// 返回新版本号。
    pub fn rollback_note(
        &self,
        title: &str,
        reference: &str,
        summary: Option<String>,
        copy: bool,
        protection: Option<Policy>,
        passphrase: Option<String>,
    ) -> Result<u64, String> {
        let rev = token_to_rev(reference)?;
        if copy {
            return self.rollback_copy(title, rev, summary);
        }

        let old = self.read_revision(title, rev)?;
        let committed = self.commit_with(title, &old.markdown, summary, protection, passphrase)?;
        Ok(committed.rev)
    }

    /// 某一版**落盘封装的细节**：签名验得怎么样、加密到谁、口令这次会话里有没有。
    ///
    /// 三样各自独立：某一层查不动（没有 gpg、外层口令还没给）只让**那一项**空着，
    /// 其余照报 —— 想知道"这一版能不能解开"的时候，不该因为验不了签名就什么都看不到。
    pub fn protection_report(
        &self,
        title: &str,
        reference: Option<&str>,
    ) -> Result<ProtectionReport, String> {
        let id = self.locate(title)?;
        let state = self.state_of(&id)?;
        let (rev, blob) = match reference {
            None => (state.rev, state.blob.clone()),
            Some(token) => {
                let rev = token_to_rev(token)?;
                let (_at, blob, _bytes, _summary) = self.event_at(&id, title, rev)?;
                (rev, blob)
            }
        };
        if blob.is_empty() {
            // 还没有正文：没有 blob，也就没有封装可报
            return Ok(ProtectionReport::default());
        }

        let protection = self.blobs().protection(&blob)?;
        let passphrase = session::passphrase_for(&id, rev);

        // 签名：逐层走进去才能验，所以外层有口令层时，得先在这次会话里解过锁
        let file = self.blobs().read_stored(&blob)?;
        let (signature, signature_problem) = match codec::signature_report(
            &file,
            &Secrets {
                passphrase: passphrase.as_deref(),
            },
        ) {
            Ok(report) => (report, None),
            Err(error) => (None, Some(error.to_string())),
        };

        // 加密：只看头里记的那把钥匙，本机认不认得
        let encryption = match &protection.encrypt {
            Some(key) => Some(codec::encryption_report(key).map_err(|error| error.to_string())?),
            None => None,
        };

        Ok(ProtectionReport {
            signature,
            signature_problem,
            encryption,
            // 口令层问的是"这次会话里有没有它的口令"，也就是"现在还读不读得动"
            passphrase_ready: protection.symmetric.then(|| passphrase.is_some()),
        })
    }

    /// 这一版的口令在不在**本次会话**里。
    ///
    /// 只看内存，不读 blob、不碰 gpg —— 界面上那枚"口令已暂存"的标记要常用，
    /// 不能顺手把验签那种花时间的活也带上。
    pub fn passphrase_stored(&self, title: &str, reference: Option<&str>) -> Result<bool, String> {
        let id = self.locate(title)?;
        let rev = match reference {
            None => self.state_of(&id)?.rev,
            Some(token) => token_to_rev(token)?,
        };
        Ok(session::passphrase_for(&id, rev).is_some())
    }

    /// 给某一版解锁：`reference` 是 token，`None` = 最新版
    pub fn unlock(
        &self,
        title: &str,
        reference: Option<&str>,
        passphrase: &str,
    ) -> Result<(), String> {
        let rev = match reference {
            Some(token) => Some(token_to_rev(token)?),
            None => None,
        };
        let (id, rev) = self.resolve_revision(title, rev)?;
        session::unlock(&id, rev, passphrase.to_string());
        Ok(())
    }
}

/// 某一版落盘封装的细节。三项各自独立，查不动的那项空着（附原因）。
#[derive(Debug, Clone, Default, Serialize)]
pub struct ProtectionReport {
    /// 签名层的校验结论；没有签名层、或验签这步没跑起来时是 `None`
    pub signature: Option<SignatureReport>,
    /// 签名没报出来的原因（没有 gpg、外层口令没给）
    pub signature_problem: Option<String>,
    /// 加密层：加密到谁、本机有没有那把私钥
    pub encryption: Option<EncryptionReport>,
    /// 口令层：这次会话里有没有这一版的口令（没套口令层就是 `None`）
    pub passphrase_ready: Option<bool>,
}

/// 版本 token → 版本号（现在只认数字；收 token 是语法层的事，解释 token 是这里的事）
fn token_to_rev(token: &str) -> Result<u64, String> {
    token
        .parse()
        .map_err(|_| format!("版本要写数字（拿到的是「{token}」）"))
}

/// 从一堆东西里随机拿一个；空集合返回 `None`
fn pick_one<T>(items: &[T]) -> Result<Option<&T>, String> {
    if items.is_empty() {
        return Ok(None);
    }

    let mut bytes = [0u8; 8];
    getrandom::getrandom(&mut bytes).map_err(|error| format!("取随机数失败：{error}"))?;
    let index = u64::from_le_bytes(bytes) as usize % items.len();
    Ok(items.get(index))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> Database {
        let dir = std::env::temp_dir().join(format!(
            "refind-note-resolve-test-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let workspace = crate::storage::workspace::Workspace::open(dir).unwrap();
        Database::open(&workspace).unwrap()
    }

    fn cleanup(database: &Database) {
        // 库在 `<暂存目录>/db` 下，settings 是它的兄弟目录 —— 从暂存根整棵删掉
        if let Some(root) = database.root().parent() {
            let _ = std::fs::remove_dir_all(root);
        }
    }

    #[test]
    fn an_existing_note_resolves_to_it() {
        let database = scratch("existing");
        database.create("示例").unwrap();

        let resolved = database.resolve_address("示例@edit#小节").unwrap().unwrap();
        assert!(matches!(resolved.outcome, Outcome::Note { ref title } if title == "示例"));
        // 这一页存在，状态与章节都保留
        assert_eq!(resolved.canonical, "示例@edit#小节");
        assert!(matches!(resolved.address.mode, Mode::Edit));

        cleanup(&database);
    }

    #[test]
    fn a_missing_note_drops_its_state() {
        let database = scratch("missing");

        let resolved = database.resolve_address("没有的@edit").unwrap().unwrap();
        assert!(matches!(resolved.outcome, Outcome::Missing { ref title } if title == "没有的"));
        assert_eq!(resolved.canonical, "没有的", "还不存在的页不保留状态");

        cleanup(&database);
    }

    #[test]
    fn special_pages_resolve_to_their_view() {
        let database = scratch("special");

        let resolved = database
            .resolve_address("special:settings#外观")
            .unwrap()
            .unwrap();
        assert!(matches!(resolved.outcome, Outcome::Special { ref page } if page == "settings"));
        assert_eq!(resolved.canonical, "Special:Settings#外观");

        cleanup(&database);
    }

    #[test]
    fn random_lands_on_a_note_and_needs_one_to_exist() {
        let database = scratch("random");
        assert!(database
            .resolve_address("special:random")
            .unwrap_err()
            .contains("还没有笔记"));

        database.create("甲").unwrap();
        database.create("乙").unwrap();
        let resolved = database.resolve_address("special:random").unwrap().unwrap();
        assert!(
            matches!(resolved.outcome, Outcome::Note { .. }),
            "{resolved:?}"
        );
        assert!(
            resolved.canonical == "甲" || resolved.canonical == "乙",
            "落点该是现有两篇之一：{}",
            resolved.canonical
        );

        cleanup(&database);
    }

    #[test]
    fn a_version_token_is_read_as_a_number() {
        let database = scratch("token");
        database.create("甲").unwrap();
        database.commit("甲", "第一版", None).unwrap();
        database.commit("甲", "第二版", None).unwrap();

        // 数字 token：读到那一版
        assert!(matches!(
            database.read_note("甲", Some("1")).unwrap(),
            Reading::Ready { note } if note.markdown == "第一版"
        ));
        // 不像数字的 token：说清楚是哪里不对
        assert!(database
            .read_note("甲", Some("abc"))
            .unwrap_err()
            .contains("要写数字"));

        cleanup(&database);
    }
}
