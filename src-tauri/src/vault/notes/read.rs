//! 读与回退：把某一版读出来、解开封装、渲染成 HTML，以及把改动退回去。
//!
//! 模板嵌入（`src=`）在读的时候要顺带把别的页也读出来，所以这一段与渲染是一起的 ——
//! 见 `template_pages` 与 `read_for_embedding`。

use std::sync::Arc;

use super::{AssembleError, Event, Note, NoteState, Reading, DEFAULT_MIME};
use crate::storage::codec::{Protection, Secrets};
use crate::storage::session;
use crate::vault::database::{now, Database};
use crate::vault::target::{PageIndex, Resolver};

/// 一页用来嵌入的正文读不出来时，带回来的两样东西。
///
/// 为什么要带保护状态：渲染的时候界面已经没机会再问后端了 —— 它得**当场**决定
/// 解锁框上要不要给口令输入框。那件事只有一个地方知道答案（这一页的封装头），
/// 所以这里把它一起带出去，而不是让界面猜。
#[derive(Debug, Clone)]
struct EmbeddedFailure {
    /// 为什么读不出来（原文，给人看）
    reason: String,
    /// 这一页怎么存的
    protection: Protection,
}

impl EmbeddedFailure {
    /// 连这一页是什么都还没定下来的失败（找不到、事件链读不出来）
    ///
    /// 那不是"这一页上了锁"，所以保护状态给"什么都没套"—— 界面于是不会摆
    /// 一个骗人的口令框，而是把 `reason` 那句话摆出来。
    fn other(reason: String) -> Self {
        Self {
            reason,
            protection: Protection::plain(),
        }
    }
}

impl Database {
    /// 读一篇笔记的**最新一版**。口令从这次会话里取。
    pub fn read(&self, title: &str) -> Result<Note, String> {
        let id = self.locate(title)?;
        let state = self.state_of(&id)?;

        self.assemble(
            &id,
            &state,
            state.rev,
            state.modified.clone(),
            state.blob.clone(),
            state.summary.clone(),
        )
        .map_err(|error| error.to_string())
    }

    /// 回滚到某一版：**解锁那一版、照这篇当前的保护重新落一份**。
    ///
    /// 那一版的内容**作为新提交**写上去，旧记录一条不改 —— 所以回滚本身也可以再被回滚。
    /// 新提交照这篇**当前**的保护，与那一版原来怎么存无关；只想"连封装一起复制"
    /// 就用 [`Self::rollback_copy`]。
    pub fn rollback(&self, title: &str, rev: u64, summary: Option<String>) -> Result<Note, String> {
        let old = self.read_revision(title, rev)?;
        self.commit(title, &old.markdown, summary)
    }

    /// 回滚（**复制那份封装**）：新提交引用**同一个 blob** —— 逐字节就是那一版，
    /// 保护也照那一版（此后这篇的"当前保护"随之变成那一版的）。不需要解锁：
    /// 不是读内容，只是把旧对象再指一次。返回新版本号。
    pub fn rollback_copy(
        &self,
        title: &str,
        rev: u64,
        summary: Option<String>,
    ) -> Result<u64, String> {
        let id = self.locate(title)?;
        let state = self.state_of(&id)?;
        let (_at, blob, bytes, _) = self.event_at(&id, title, rev)?;
        let new_rev = state.rev + 1;

        self.append(
            &id,
            &Event::Rev {
                at: now(),
                rev: new_rev,
                blob,
                bytes,
                mime: DEFAULT_MIME.to_string(),
                summary,
            },
        )?;

        // 解过那一版的话，把口令顺延给新版本 —— 复制之后照样读得出来
        if let Some(passphrase) = session::passphrase_for(&id, rev) {
            session::unlock(&id, new_rev, passphrase);
        }

        Ok(new_rev)
    }

    /// 读某一版。版本从 1 开始连续编号。
    ///
    /// 存储状态取**那一版自己的** blob 头 —— 它按当时的策略封装，与最新一版未必相同。
    pub fn read_revision(&self, title: &str, rev: u64) -> Result<Note, String> {
        let id = self.locate(title)?;
        let (at, blob, _, summary) = self.event_at(&id, title, rev)?;
        let state = self.state_of(&id)?;

        self.assemble(&id, &state, rev, at, blob, summary)
            .map_err(|error| error.to_string())
    }

    /// 交给界面读**最新一版**：上了锁就明说。
    ///
    /// 读不到不是错误 —— 界面得知道"这是缺口令"，才好把人带到输入口令那一页去，
    /// 而不是丢一个看不懂的错。
    pub fn read_for_display(&self, title: &str) -> Result<Reading, String> {
        let id = self.locate(title)?;
        let state = self.state_of(&id)?;

        if let Some(protection) = self.lock_of(&id, state.rev, &state.blob)? {
            return Ok(Reading::Locked {
                protection,
                reason: "这一篇是加密存的，输入口令后显示".to_string(),
                wrong_passphrase: false,
            });
        }

        let assembled = self.assemble(
            &id,
            &state,
            state.rev,
            state.modified.clone(),
            state.blob.clone(),
            state.summary.clone(),
        );

        match assembled {
            Ok(note) => Ok(Reading::Ready { note }),
            // **任何"解不开"都归成 Locked**，不报原始错误。
            //
            // 这一支原来是 `is_wrong_passphrase`（在字符串里找"口令不对"那句话），
            // 所以 **gpg 解不开的笔记走的是 `Err`** —— 人看到的是一句技术话，
            // 没有框、没有重试，而它恰恰是最该给框的那一种（点一下就能让 gpg 去问
            // 钥匙串/智能卡）。口令错了与 gpg 失败在这里合流，区别只在 `reason`
            // 与 `wrong_passphrase` 两个字段里说。
            Err(error) if error.is_lock() => {
                let wrong_passphrase = error.is_wrong_passphrase();
                if wrong_passphrase {
                    // 口令不对：丢掉**这一版**的那把，这样界面把人带回去时还能重新输
                    session::forget(&id, state.rev);
                }
                Ok(Reading::Locked {
                    // 保护头读不到就报出去 —— 编一个"没加密"的默认值
                    // 会让界面摆出一个**没有输入框**的解锁框，而真相是文件坏了
                    protection: self.blobs().protection(&state.blob)?,
                    reason: if wrong_passphrase {
                        "口令不对，再输一次".to_string()
                    } else {
                        error.to_string()
                    },
                    wrong_passphrase,
                })
            }
            Err(error) => Err(error.to_string()),
        }
    }

    /// 交给界面读**某一版**：同一套判断
    pub fn read_revision_for_display(&self, title: &str, rev: u64) -> Result<Reading, String> {
        let id = self.locate(title)?;
        let (at, blob, _, summary) = self.event_at(&id, title, rev)?;

        if let Some(protection) = self.lock_of(&id, rev, &blob)? {
            return Ok(Reading::Locked {
                protection,
                reason: "这一版是加密存的，输入口令后显示".to_string(),
                wrong_passphrase: false,
            });
        }

        let state = self.state_of(&id)?;
        // 保护头要读**这一版**的 blob：`assemble` 会把它拿走，所以先留一份
        match self.assemble(&id, &state, rev, at, blob.clone(), summary) {
            Ok(note) => Ok(Reading::Ready { note }),
            Err(error) if error.is_lock() => {
                let wrong_passphrase = error.is_wrong_passphrase();
                if wrong_passphrase {
                    session::forget(&id, rev);
                }
                Ok(Reading::Locked {
                    // 同上：读不到就报出去，不编默认值
                    protection: self.blobs().protection(&blob)?,
                    reason: if wrong_passphrase {
                        "口令不对，再输一次".to_string()
                    } else {
                        error.to_string()
                    },
                    wrong_passphrase,
                })
            }
            Err(error) => Err(error.to_string()),
        }
    }

    /// 某一版在日志里的那一行：`(提交时间, blob, 字节数, 说明)`
    pub(crate) fn event_at(
        &self,
        id: &str,
        title: &str,
        rev: u64,
    ) -> Result<(String, String, u64, Option<String>), String> {
        let event = self
            .read_events(id)?
            .into_iter()
            .find(|event| matches!(event, Event::Rev { rev: number, .. } if *number == rev))
            .ok_or_else(|| format!("「{title}」没有第 {rev} 版"))?;

        let Event::Rev {
            at,
            blob,
            bytes,
            summary,
            ..
        } = event
        else {
            return Err(format!("「{title}」没有第 {rev} 版"));
        };

        Ok((at, blob, bytes, summary))
    }

    /// 这一版是不是"套了口令层、但这次会话还没给这一页口令"。
    ///
    /// 口令按页存，所以这里必须按 `id` 问 —— 别的页解过锁不算。
    fn lock_of(&self, id: &str, rev: u64, blob: &str) -> Result<Option<Protection>, String> {
        if blob.is_empty() {
            return Ok(None);
        }

        let protection = self.blobs().protection(blob)?;
        if protection.symmetric && session::passphrase_for(id, rev).is_none() {
            return Ok(Some(protection));
        }

        Ok(None)
    }

    /// 把一版装配成 [`Note`]：正文从 blob 解出来、渲染成 HTML，
    /// 存储状态从 blob 的**明文头**读。
    fn assemble(
        &self,
        id: &str,
        state: &NoteState,
        rev: u64,
        modified: String,
        blob: String,
        summary: Option<String>,
    ) -> Result<Note, AssembleError> {
        let display = state.title.clone();

        let (markdown, protection) = if blob.is_empty() {
            // 还没有正文（0 版），也就无所谓封装
            (
                String::new(),
                Protection {
                    compress: false,
                    compression: crate::storage::codec::Compression::default(),
                    sign: None,
                    encrypt: None,
                    symmetric: false,
                    cipher: crate::storage::codec::Cipher::default(),
                },
            )
        } else {
            let passphrase = session::passphrase_for(id, rev);
            // **类型化**的那一条：要分辨"上了锁"与"坏了"，字符串里分不出来
            let bytes = self.blobs().get_typed(
                &blob,
                &Secrets {
                    passphrase: passphrase.as_deref(),
                },
            )?;
            let markdown = String::from_utf8(bytes)
                .map_err(|_| AssembleError::Other(format!("「{display}」不是文本")))?;

            (
                markdown,
                self.blobs()
                    .protection(&blob)
                    .map_err(crate::storage::codec::CodecError::Corrupt)?,
            )
        };

        // 指令页面（`$$COMMAND$$` 打头）**按原文看**：包成代码块再渲染，
        // 于是"这一页是指令"一眼看得出来，而不是被当成正文读了过去
        let parsed = crate::vault::command::parse(&markdown);
        let command = crate::vault::command::CommandInfo::from_parsed(&parsed);
        let html = if command.is_some() {
            let resolver = self.resolver_for(&display);
            crate::markdown::render_with(&crate::markdown::fence_code(&markdown), Some(&resolver))
        } else {
            self.render_html(&markdown, &display)
                .map_err(AssembleError::Other)?
        };

        Ok(Note {
            key: format!("{}:{}", state.ns, state.title),
            command,
            title: display,
            markdown,
            html,
            rev,
            created: state.created.clone(),
            modified,
            summary,
            protection,
        })
    }

    /// 把正文渲染成 HTML。
    ///
    /// 内部链接要判红蓝，所以得先把"现有页面"的索引交给解析器 ——
    /// 渲染发生在 markdown 的回调里，那时没有仓库可查。
    /// 那份索引与地址解析用的是同一个来源（见 [`PageIndex::of`]）。
    ///
    /// 阅读页与编辑器预览都走这里：**渲染只有一处**，所以"预览里是什么样"
    /// 与"存下来再读是什么样"不会分家。
    pub fn render_html(&self, markdown: &str, title: &str) -> Result<String, String> {
        let resolver = self.resolver_for(title);
        Ok(crate::markdown::render_with_pages(
            markdown,
            Some(&resolver),
            Some(std::sync::Arc::new(self.template_pages(markdown))),
        ))
    }

    /// `Template:` 里每一页 —— 模板的 `src=` 要把另一页嵌进来（见 `markdown::TemplatePages`）。
    ///
    /// 两条规矩：
    ///
    /// 1. **只在正文里出现 `src=` 或 `::` 时才去取** —— 每次渲染都把模板页读一遍太亏，
    ///    而这两种记号必须原样写在正文里，漏不掉（认错的代价只是白读几页）；
    /// 2. **绝不弹口令**（见 [`codec::without_prompting`]）—— 渲染是"顺手看一眼"，
    ///    不该因为某一页没解锁就把界面挂在一个口令框上。取不到就白纸黑字写清楚，
    ///    让作者自己决定去不去解锁（打开那一页一次，口令就进了这一趟的缓存）。
    fn template_pages(
        &self,
        markdown: &str,
    ) -> std::collections::HashMap<String, crate::markdown::TemplatePage> {
        use crate::markdown::TemplatePage;

        let mut pages = std::collections::HashMap::new();
        // 两种用法都要把表备好：`src=`（把一页当片段嵌进来）与 `::名字`（用户模板）。
        // 两个记号都必须原样出现在正文里，漏不掉；认错的代价只是白读几页。
        if !markdown.contains("src=") && !markdown.contains("::") {
            return pages;
        }

        let table = self.namespaces();
        let Ok(titles) = self.titles() else {
            return pages;
        };

        for title in titles.notes.values() {
            let Ok(parsed) = crate::vault::title::parse(title, &table) else {
                continue;
            };
            if parsed.ns != crate::vault::namespace::TEMPLATE_ID {
                continue;
            }

            let page = match self.read_for_embedding(title) {
                Ok(text) => TemplatePage::Ready(text),
                Err(failure) => TemplatePage::Unreadable {
                    reason: failure.reason,
                    protection: failure.protection,
                },
            };
            pages.insert(crate::markdown::template_page_key(&parsed.page), page);
        }

        pages
    }

    /// 读一页用来嵌入的正文：**不弹口令**，而且这一趟读过就不再读。
    ///
    /// 缓存键是"标题 + 版本号"：那一页提交了新版本，缓存自然失效；
    /// 而编辑器预览每敲一个字就重渲染一次，没有这一层缓存就是每次几十次解密。
    fn read_for_embedding(&self, title: &str) -> Result<String, EmbeddedFailure> {
        let id = self.locate(title).map_err(EmbeddedFailure::other)?;
        let state = self.state_of(&id).map_err(EmbeddedFailure::other)?;
        let rev = state.rev;

        if let Some((cached_rev, text)) = session::cached_page(title) {
            if cached_rev == rev {
                return Ok(text);
            }
        }

        // 头是明文：**先看它加没加密**，好把"没解锁"和"真出错"分开说
        let encrypted = self
            .blobs()
            .protection(&state.blob)
            .map(|protection| protection.symmetric || protection.encrypt.is_some())
            .unwrap_or(false);

        // 保护状态就是那一句 `encrypted` 的根据，读出来一并带走 ——
        // 渲染的时候界面要靠它决定要不要给口令输入框，那时已经没机会再问了。
        let protection = self
            .blobs()
            .protection(&state.blob)
            .unwrap_or_else(|_| Protection::plain());
        let locked = || format!("《{title}》是加密的：解锁之后，模板就能嵌进来了");

        // **只读字节，不渲染** —— 这一条最要紧：`read_note` 会顺手把 HTML 也渲染出来，
        // 而渲染又要读模板页（就是这里）→ 自己套自己，栈直接爆掉（"打开就卡死"）。
        // 嵌入要的是"那一页写了什么"，不是"它渲染成什么样"。
        let read = crate::storage::codec::without_prompting(|| self.read_bytes(title, None));
        match read {
            Ok((bytes, _mime)) => {
                let text = String::from_utf8(bytes).map_err(|_| EmbeddedFailure {
                    reason: format!("《{title}》不是文本，嵌不进来"),
                    protection: Protection::plain(),
                })?;
                session::cache_page(title, rev, text.clone());
                Ok(text)
            }
            // 加密的页面读失败，多半就是没解锁（gpg 那边拿不到口令）
            Err(_) if encrypted => Err(EmbeddedFailure {
                reason: locked(),
                protection,
            }),
            Err(error) => Err(EmbeddedFailure {
                reason: format!("《{title}》读不出来：{error}"),
                protection,
            }),
        }
    }

    /// 给"当前页是某一页"的一次渲染装配解析器（红蓝链、`[[/子页]]` 都靠它）
    fn resolver_for(&self, title: &str) -> Resolver {
        let table = Arc::new(self.namespaces());
        let from = crate::vault::title::parse(title, &table).ok();
        let index = Arc::new(PageIndex::of(self));
        Resolver::new(table, index, from)
    }
}
