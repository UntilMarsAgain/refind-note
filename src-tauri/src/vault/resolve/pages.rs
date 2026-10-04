//! **地址 → 哪一页**。
//!
//! 这一层把地址栏里那行字与当前仓库的内容对上，给出一个 [`Outcome`]：这一页在不在、
//! 是哪种地方、界面能不能改。特殊页与跨站页的裁剪、指令页的跟跳（含 [`MAX_HOPS`] 的
//! 环保护）、`special:random` 的落点、以及"随机挑一篇"都在这里。
//!
//! 两条规矩贯穿全文件：
//!
//! - **状态只在"这一页存在"的那一支上有意义** —— 还不存在的页、跨站页、帮助页，
//!   状态与章节都裁掉，回显出来的地址就是光秃秃的页名；
//! - **`editable` 由仓库说了算** —— 文件页与特殊页不可改，界面照它摆按钮，不自己特判。
//!
//! 正文读不读得动不在这里管：那是 [`crate::vault::notes`] 的事（见 [`Database::read_note`]）。

use super::{Outcome, ResolvedAddress, Via, MAX_HOPS};
use crate::vault::address::{self, Address, Mode, ParsedAddress};
use crate::vault::command;
use crate::vault::database::Database;
use crate::vault::namespace::SPECIAL_ID;
use crate::vault::notes::Reading;
use crate::vault::title::ParsedTitle;

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
