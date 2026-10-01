//! 地址的**语义解析**：把语法解析的结果落到仓库上。
//!
//! 语法（前缀、状态词、章节）在 `address` 里；这里回答的是"这一页在不在、是哪种地方"：
//! 特殊页、还不存在的页、随机跳转，以及把版本 token 解释成版本号。
//! 内容本身（含上锁状态）由 [`Database::read_note`] 走 `notes` 那套。

use serde::Serialize;

use crate::storage::codec::{self, EncryptionReport, Policy, Secrets, SignatureReport};
use crate::storage::session;
use crate::vault::address::{self, Address, Mode, ParsedAddress};
use crate::vault::command;
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
    /// 帮助页（虚拟命名空间 `Help`）：页面随程序发布，不在仓库里
    Help { page: String, title: String },
    /// 跨站命名空间里的页面：本仓库没有它，地址在 `url`
    CrossSite { title: String, url: String },
    /// 文件页面（`File:桥.png`）：正文是字节，不是给人读的文本
    File { title: String },
}

/// 这一页是**被哪条指令带过来的**（`$$COMMAND$$` 那一页）
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Via {
    /// 发起跳转的那一页（显示标题）；随机跳转时也会给，界面可以选择不显示
    pub from: String,
    /// 是随机跳转（提示语不写具体名字）
    pub random: bool,
}

/// 跟跳的上限。
///
/// 这是**仓库的跟跳策略**，不是指令语法（语法在 [`crate::vault::command`]）——
/// 指令写成了环时，给一句能看懂的提示，总好过递归到栈溢出。
const MAX_HOPS: usize = 8;

/// 地址 + 它落到仓库上的结论
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ResolvedAddress {
    pub address: Address,
    pub canonical: String,
    pub outcome: Outcome,
    /// **这一页能不能改**。由仓库说了算 —— 界面照它决定摆哪些按钮，
    /// 而不是自己按页面种类特判（帮助页不可改、将来的文件页也不可改）。
    pub editable: bool,
    /// 被指令带过来时才有的"从哪儿来"
    pub via: Option<Via>,
}

impl Database {
    /// 解析地址栏那一行并落到仓库上；空输入不是地址（`None`）。
    ///
    /// - 特殊页：`special:random` 是**一次跳转**，挑一篇笔记落下去（返回目标那篇的结果）；
    /// - 还不存在的页：状态裁掉（`名称@edit` 与 `名称` 回显成同一个地址）——
    ///   状态只在"这一页存在"的那一支上有意义。
    pub fn resolve_address(&self, input: &str) -> Result<Option<ResolvedAddress>, String> {
        self.resolve_address_at(input, 0, None)
    }

    /// 同上，但带上"已经跟了几跳"与"从哪儿来"（指令页跟跳时递归用）
    fn resolve_address_at(
        &self,
        input: &str,
        hops: usize,
        via: Option<Via>,
    ) -> Result<Option<ResolvedAddress>, String> {
        let table = self.namespaces();
        let Some(parsed) = address::parse(input, &table)? else {
            return Ok(None);
        };
        let ParsedAddress { address, canonical } = parsed;

        let index = crate::vault::target::PageIndex::of(self);

        // 跨站命名空间：本仓库没有这一页，地址由命名空间的模板拼出来 ——
        // 与正文里 `[[zhwiki:条目]]` 那条绿链走的是同一处规则（`target::resolve`）
        if let Some(found) = table
            .get(&address.namespace.id)
            .filter(|item| item.is_cross_site())
        {
            let page = address.page.clone();
            let Some(url) = found.url_for(&page) else {
                return Err(format!("「{}」没有配站点地址", found.name));
            };
            // 状态与章节在别人家的页面上没有意义：裁掉，回显出来的就是那个地址
            let address = Address {
                mode: Mode::View { reference: None },
                ..address
            };
            let canonical = address::compose(&address);
            return Ok(Some(ResolvedAddress {
                address,
                canonical,
                outcome: Outcome::CrossSite {
                    title: format!("{}:{page}", found.name),
                    url,
                },
                // 别人家的页面：本程序改不了
                editable: false,
                via,
            }));
        }

        if address.namespace.id == crate::vault::namespace::HELP_ID {
            let wanted = address.page.trim();
            let Some(found) = crate::features::help::find(self, wanted) else {
                return Err(format!("没有这页帮助：{wanted}"));
            };

            // 帮助页只有"看"与"看源码"两种状态；别的（历史、删除、回退…）在帮助上
            // 没有意义，静默裁掉 —— 与特殊页面同一条规矩
            let mode = match address.mode {
                Mode::Edit => Mode::Edit,
                _ => Mode::View { reference: None },
            };
            let address = Address {
                page: found.slug.clone(),
                mode,
                ..address
            };
            let canonical = address::compose(&address);
            return Ok(Some(ResolvedAddress {
                address,
                canonical,
                outcome: Outcome::Help {
                    page: found.slug.clone(),
                    title: found.display.clone(),
                },
                // 帮助随程序发布：这里改不了（`@edit` 是看源码）
                editable: false,
                via,
            }));
        }

        if address.namespace.id == SPECIAL_ID {
            if address.page.eq_ignore_ascii_case("random") {
                return self.resolve_random(hops, via);
            }
            let page = address.page.to_lowercase();
            return Ok(Some(ResolvedAddress {
                address,
                canonical,
                outcome: Outcome::Special { page },
                // 特殊页面是程序自己的界面，没有"改它的正文"这回事
                editable: false,
                via,
            }));
        }

        // 给出去的是**显示标题**（命名空间写成规范名）：前端拿它拼地址、让后端读，
        // 与人在地址栏里敲 `帮助:入门` 是同一件事 —— 别名在这里收敛成规范名
        let title = ParsedTitle {
            ns: address.namespace.id.clone(),
            page: address.page.clone(),
        }
        .display(&table);

        // "在不在"问的就是链接解析那张索引：一处回答，两处一样
        if index.contains(&address.namespace.id, &address.page) {
            // 文件页面：正文是字节。它也有历史、也能删，但没有"编辑正文"这回事，
            // 所以 `@edit` 裁成阅读（改了正文的编辑从哪里来？从"传新版"来）
            if address.namespace.id == crate::vault::namespace::FILE_ID {
                let mode = match address.mode {
                    Mode::View { reference } => Mode::View { reference },
                    Mode::History => Mode::History,
                    Mode::Delete => Mode::Delete,
                    _ => Mode::View { reference: None },
                };
                let address = Address { mode, ..address };
                let canonical = address::compose(&address);
                return Ok(Some(ResolvedAddress {
                    address,
                    canonical,
                    outcome: Outcome::File { title },
                    // 正文（字节）不在这里改；改名与传新版另有其路
                    editable: false,
                    via,
                }));
            }

            // 指令页面：**只有"看最新版"这一路跟跳**。
            // 编辑 / 历史 / 删除 / 看旧版操作的都是这一页本身，跟着跳走会让人改错页面。
            if matches!(address.mode, Mode::View { reference: None }) {
                if let Some((target, random)) = self.command_target(&title, hops)? {
                    return self.resolve_address_at(
                        &target,
                        hops + 1,
                        Some(Via {
                            from: title,
                            random,
                        }),
                    );
                }
            }

            return Ok(Some(ResolvedAddress {
                address,
                canonical,
                outcome: Outcome::Note { title },
                // 仓库里的笔记：改得了
                editable: true,
                via,
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
            // 还不存在：建起来就是一篇可改的笔记
            editable: true,
            via,
        }))
    }

    /// 这一页是不是指令页；是的话跳到哪
    ///
    /// 读不动（上了锁、内容坏了）就当不是 —— 那两种情况本来就有各自的页面要显示。
    fn command_target(&self, title: &str, hops: usize) -> Result<Option<(String, bool)>, String> {
        let markdown = match self.read_note(title, None) {
            Ok(Reading::Ready { note }) => note.markdown,
            _ => return Ok(None),
        };

        match command::parse(&markdown) {
            // 不是指令页面：照常阅读
            command::Parsed::None => Ok(None),
            command::Parsed::Command(found) => {
                if hops >= MAX_HOPS {
                    return Err(format!(
                        "「{title}」的跳转绕成了环（跟了 {MAX_HOPS} 跳还没到头）"
                    ));
                }
                let random = found.spec.kind == "random-redirect";
                match found
                    .chase(self, title)
                    .map_err(|message| format!("「{title}」：{message}"))?
                {
                    Some(target) => Ok(Some((target, random))),
                    // 表里标明"不跳"的指令：当普通页面读
                    None => Ok(None),
                }
            }
            // 是指令页面，但指令本身有问题 —— **不能当普通页面读**：
            // 那样一条写坏的指令会静静显示成正文，谁也不知道它没生效
            command::Parsed::Empty => Err(format!(
                "「{title}」是指令页面，但没写指令（第二行应写成 {}）",
                command::supported()
            )),
            command::Parsed::Unrecognized(line) => Err(format!(
                "「{title}」的指令认不出来：「{}」；目前支持 {}",
                line.trim(),
                command::supported()
            )),
        }
    }

    /// `special:random`：挑一篇笔记，落到它身上
    fn resolve_random(
        &self,
        hops: usize,
        via: Option<Via>,
    ) -> Result<Option<ResolvedAddress>, String> {
        let Some(title) = self.pick_title(None, None)? else {
            return Err("仓库里还没有笔记，随机跳转没地方去".to_string());
        };
        let via = via.or(Some(Via {
            from: "special:random".to_string(),
            random: true,
        }));
        self.resolve_address_at(&title, hops + 1, via)
    }

    /// 在某个命名空间里随机挑一篇的显示标题。
    ///
    /// `namespace` 是**命名空间的名字或别名**（`None` 或空串 = 主命名空间）；
    /// `exclude` 是要排除的那一篇（随机跳转不该跳回自己）。
    pub fn pick_title(
        &self,
        namespace: Option<&str>,
        exclude: Option<&str>,
    ) -> Result<Option<String>, String> {
        let table = self.namespaces();
        let wanted = match namespace.map(str::trim).filter(|name| !name.is_empty()) {
            None => crate::vault::namespace::MAIN_ID.to_string(),
            Some(name) => table
                .lookup(name)
                .map(|item| item.id.clone())
                .ok_or_else(|| format!("没有这个命名空间：{name}"))?,
        };

        let mut titles: Vec<String> = self
            .list()?
            .into_iter()
            .filter(|note| note.key.starts_with(&format!("{wanted}:")))
            .map(|note| note.title)
            .collect();
        if let Some(exclude) = exclude {
            titles.retain(|title| title != exclude);
        }

        Ok(pick_one(&titles)?.cloned())
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

/// 把仓库的随机能力交给指令表：指令只知道"要随机挑一篇"，怎么挑是这里的事。
impl command::CommandEnv for Database {
    fn random_title(&self, namespace: Option<&str>, from: &str) -> Result<String, String> {
        self.pick_title(namespace, Some(from))?
            .ok_or_else(|| "那个命名空间里没有别的页面可跳".to_string())
    }
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

    /// 写一篇指令页
    fn command_page(database: &Database, title: &str, body: &str) {
        database.create(title).unwrap();
        database
            .commit(title, &format!("$$COMMAND$$\n{body}\n"), None)
            .unwrap();
    }

    /// 打开指令页 → 落到它指向的那一页，并且记得"从哪儿来"
    #[test]
    fn a_command_page_chases_to_its_target() {
        let database = scratch("chase");
        database.create("目标").unwrap();
        database.commit("目标", "正文", None).unwrap();
        command_page(&database, "跳板", "REDIRECT: 目标");

        let resolved = database.resolve_address("跳板").unwrap().unwrap();
        assert_eq!(
            resolved.outcome,
            Outcome::Note {
                title: "目标".to_string()
            }
        );
        assert_eq!(
            resolved.via,
            Some(Via {
                from: "跳板".to_string(),
                random: false
            })
        );

        // `@no-command`：不跟跳，看这一页自己
        let kept = database
            .resolve_address("跳板@no-command")
            .unwrap()
            .unwrap();
        assert_eq!(
            kept.outcome,
            Outcome::Note {
                title: "跳板".to_string()
            }
        );
        assert_eq!(kept.via, None);

        // 编辑、看历史这些状态也不跟跳 —— 否则会改错页面
        for state in ["跳板@edit", "跳板@history", "跳板@delete"] {
            let resolved = database.resolve_address(state).unwrap().unwrap();
            assert_eq!(
                resolved.outcome,
                Outcome::Note {
                    title: "跳板".to_string()
                },
                "{state} 不该跟着跳"
            );
        }

        cleanup(&database);
    }

    /// 读指令页：正文按**代码块**看，并带上指令信息
    #[test]
    fn a_command_page_reads_as_a_code_block_with_its_info() {
        let database = scratch("command-read");
        command_page(&database, "跳板", "REDIRECT: 别处");

        let Reading::Ready { note } = database
            .read_note("跳板@no-command".trim_end_matches("@no-command"), None)
            .unwrap()
        else {
            panic!("应当读得到");
        };
        let info = note.command.as_ref().expect("应当有指令信息");
        assert_eq!(info.kind, "redirect");
        assert_eq!(info.label, "重定向");
        assert_eq!(info.argument, "别处");
        assert!(
            note.html.contains("<pre") || note.html.contains("<code"),
            "指令页的正文应当按代码块渲染：{}",
            note.html
        );

        cleanup(&database);
    }

    /// 跳转绕成环：报一句能看懂的错，而不是递归到栈溢出
    #[test]
    fn a_redirect_loop_is_reported() {
        let database = scratch("loop");
        command_page(&database, "甲", "REDIRECT: 乙");
        command_page(&database, "乙", "REDIRECT: 甲");

        let error = database.resolve_address("甲").unwrap_err();
        assert!(error.contains("环"), "{error}");

        cleanup(&database);
    }

    /// 指令写坏了要报出来 —— 不能静静显示成正文
    #[test]
    fn a_broken_command_is_reported() {
        let database = scratch("broken");
        command_page(&database, "坏的", "随便写点什么");

        let error = database.resolve_address("坏的").unwrap_err();
        assert!(error.contains("认不出来"), "{error}");

        // 只有标记、第二行都没写
        database.create("空的").unwrap();
        database.commit("空的", "$$COMMAND$$", None).unwrap();
        let error = database.resolve_address("空的").unwrap_err();
        assert!(error.contains("没写指令"), "{error}");

        cleanup(&database);
    }

    /// 随机重定向：落在指定命名空间里的某一篇，并且标明这是随机来的
    #[test]
    fn random_redirect_lands_somewhere_in_that_namespace() {
        let database = scratch("random-command");
        database.add_namespace("manual", Vec::new(), None).unwrap();
        database.create("manual:甲").unwrap();
        database.create("manual:乙").unwrap();
        command_page(&database, "随机", "RANDOM_REDIRECT: manual");

        let resolved = database.resolve_address("随机").unwrap().unwrap();
        let Outcome::Note { title } = &resolved.outcome else {
            panic!("应当落到一篇笔记上");
        };
        assert!(title == "manual:甲" || title == "manual:乙", "{title}");
        let via = resolved.via.expect("随机跳转也要记下从哪儿来");
        assert!(via.random, "界面据此说「来自随机跳转」");

        cleanup(&database);
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
