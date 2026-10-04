//! 建与提交：新建一篇、写入一版、改标题，以及按字节存取。
//!
//! 提交要决定**这一版的封装策略**，而策略是笔记的属性而不是每次调用的参数 —— 不给策略
//! 时它从这篇当前最新的那一版继承，不会因为某次忘了传就偷偷降级。要改就显式给一次。

use super::{next_id, Event, Note, NoteState, DEFAULT_MIME};
use crate::storage::codec::{Meta, Policy, Secrets};
use crate::storage::session;
use crate::storage::workspace::append_line;
use crate::vault::database::{now, Database};

impl Database {
    ///
    /// 名字与地址解析走同一套规整与词法检查：存进去的必须是**能被解析回来**的形状，
    /// 否则就成了"建得出来、却打不开"的笔记。
    pub fn create(&self, title: &str) -> Result<String, String> {
        let table = self.namespaces();
        let parsed = crate::vault::title::parse(title, &table)?;
        let display = parsed.display(&table);

        let mut titles = self.titles()?;
        if self.exists(&display) {
            return Err(format!("已经有一篇叫「{display}」的笔记"));
        }

        // 命名空间是刚建的、或目录被人删了：写之前先把目录备好
        self.ensure_namespace_dir(&parsed.ns)?;

        let id = next_id(&titles);
        let line = serde_json::to_string(&Event::Meta {
            at: now(),
            ns: parsed.ns.clone(),
            title: display.clone(),
        })
        .map_err(|error| format!("写不进日志：{error}"))?;
        append_line(&self.note_path(&parsed.ns, &id), &line)?;

        titles.notes.insert(id, display.clone());
        self.save_titles(&titles)?;
        Ok(display)
    }

    /// 标题 + 可选版本号 → (标识, 版本号)。
    ///
    /// 不给版本就是最新一版 —— `@unlock` 与 `@unlock-3` 都归它收口。
    pub fn resolve_revision(&self, title: &str, rev: Option<u64>) -> Result<(String, u64), String> {
        let id = self.locate(title)?;
        let state = self.state_of(&id)?;
        Ok((id, rev.unwrap_or(state.rev)))
    }

    /// 提交一版正文。封装照**这篇笔记当前的保护**（见 [`Self::commit_with`]）。
    pub fn commit(
        &self,
        title: &str,
        markdown: &str,
        summary: Option<String>,
    ) -> Result<Note, String> {
        self.commit_with(title, markdown, summary, None, None)
    }

    /// 这篇笔记**当前的保护**：最新一版落盘时的封装；还没有正文时用仓库默认。
    ///
    /// 提交与草稿都从它出发 —— 保护是文件的属性，只在显式换保护时变化。
    // 草稿槽位也要用同一套"从这篇当前保护出发"的推断（见 `drafts::save_draft`），
    // 所以这里得让兄弟模块够得着。
    pub(super) fn current_policy(&self, state: &NoteState) -> Result<Policy, String> {
        if state.blob.is_empty() {
            return Ok(self.protection());
        }

        let protection = self.blobs().protection(&state.blob)?;
        Ok(Policy {
            compress: protection.compress,
            compression: protection.compression,
            gpg_sign: protection.sign,
            gpg_encrypt: protection.encrypt,
            symmetric: protection.symmetric,
            cipher: protection.cipher,
        })
    }

    /// 提交一版正文。
    ///
    /// **不给封装就照这篇笔记当前的保护**（最新一版的封装；还没有正文时用仓库默认）——
    /// 一篇笔记一旦用某种封装写过，之后的提交都照它来，不会因为某次没写参数而偷偷降级。
    /// **显式给一次**就是换保护，从这一版起照新的粘住。
    ///
    /// 口令：显式给的优先；否则用会话里**最新一版**的那把 —— 加密的笔记不该因为
    /// "新版本号还没有口令"而写不进去（回滚也走这条路）。写成之后把口令顺延给新版本，
    /// 于是"解锁一次、读写全通"。
    pub fn commit_with(
        &self,
        title: &str,
        markdown: &str,
        summary: Option<String>,
        protection: Option<Policy>,
        passphrase: Option<String>,
    ) -> Result<Note, String> {
        self.commit_bytes(
            title,
            markdown.as_bytes(),
            DEFAULT_MIME,
            summary,
            protection,
            passphrase,
        )?;
        self.read(title)
    }

    /// 提交**任意字节**（文件页面走这条：正文不是文本，但它同样是一版内容），
    /// 返回**新版本号**。
    ///
    /// 与文本提交共用同一条路 —— 于是版本、封装、回收站、整理都自动接上，
    /// 区别只在 `mime` 与"读出来是字节而不是文本"。
    ///
    /// 它**不拼 `Note`**：那是"读出来给人看的"，而这里可能压根不是文本
    /// （`String::from_utf8` 一失败就报"不是文本"—— 上传一张图也会撞上）。
    /// 要文本用 [`Self::read`]，要字节用 [`Self::read_bytes`]。
    pub fn commit_bytes(
        &self,
        title: &str,
        content: &[u8],
        mime: &str,
        summary: Option<String>,
        protection: Option<Policy>,
        passphrase: Option<String>,
    ) -> Result<u64, String> {
        let id = self.locate(title)?;
        let state = self.state_of(&id)?;
        let rev = state.rev + 1;

        let policy = match protection {
            Some(policy) => policy,
            None => self.current_policy(&state)?,
        };
        let passphrase = passphrase.or_else(|| session::passphrase_for(&id, state.rev));

        let blob = self.blobs().put(
            content,
            &Meta {
                mime: mime.to_string(),
            },
            &policy,
            passphrase.as_deref(),
        )?;

        // 写成了才把口令顺延给新版本：解锁一次，读写全通
        if let Some(passphrase) = passphrase {
            session::unlock(&id, rev, passphrase);
        }

        self.append(
            &id,
            &Event::Rev {
                at: now(),
                rev,
                blob,
                bytes: content.len() as u64,
                mime: mime.to_string(),
                summary,
            },
        )?;

        Ok(rev)
    }

    /// 读一篇**文件页面**的最新一版：字节 + 它自述的 mime。
    ///
    /// 与 [`Self::read`] 分开，是因为正文页面读出来必须是文本（不是文本就是坏了），
    /// 而文件页面读出来本来就是字节。
    pub fn read_bytes(
        &self,
        title: &str,
        reference: Option<&str>,
    ) -> Result<(Vec<u8>, String), String> {
        let id = self.locate(title)?;
        let (rev, blob, mime) = match reference {
            None => {
                let state = self.state_of(&id)?;
                let mime = if state.blob.is_empty() {
                    String::new()
                } else {
                    self.blobs().inspect(&state.blob)?.meta.mime
                };
                (state.rev, state.blob, mime)
            }
            Some(token) => {
                let rev: u64 = token
                    .parse()
                    .map_err(|_| format!("版本要写数字（拿到的是「{token}」）"))?;
                let (_at, blob, _bytes, _summary) = self.event_at(&id, title, rev)?;
                let mime = self.blobs().inspect(&blob)?.meta.mime;
                (rev, blob, mime)
            }
        };
        if blob.is_empty() {
            return Err(format!("「{title}」还没有内容"));
        }

        let passphrase = session::passphrase_for(&id, rev);
        let bytes = self.blobs().get(
            &blob,
            &Secrets {
                passphrase: passphrase.as_deref(),
            },
        )?;
        Ok((bytes, mime))
    }

    /// 改名：标题变了，别的一个不动。
    ///
    /// 两处都要写 —— 日志里留一条 `Rename`（否则重放会把旧名字读回来），
    /// `titles.json` 换成新显示标题（查表走的是它）。
    pub fn rename(&self, title: &str, new_title: &str) -> Result<String, String> {
        let id = self.locate(title)?;
        let table = self.namespaces();
        let parsed = crate::vault::title::parse(new_title, &table)?;
        let display = parsed.display(&table);

        if display != self.display_of(title)? && self.exists(&display) {
            return Err(format!("已经有一篇叫「{display}」的笔记"));
        }

        self.append(
            &id,
            &Event::Rename {
                at: now(),
                title: display.clone(),
            },
        )?;

        let mut titles = self.titles()?;
        titles.notes.insert(id, display.clone());
        self.save_titles(&titles)?;
        Ok(display)
    }
}
