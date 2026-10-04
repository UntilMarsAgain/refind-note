//! **哪一版 → 正文**。
//!
//! 版本 token 一旦定下来（[`token_to_rev`]），剩下的问题就都是"这一版的正文与封装
//! 怎么样"：读、导出、回滚到它、给它解锁、以及逐层报告它落盘时封了什么。都在这里。
//!
//! 与 [`pages`] 的分工：那边回答"落到哪一页"，这边回答"落到**第几版**"。这一层
//! **不生成 HTML**（渲染在 `markdown` 那边），也**不碰字节**（加解密与封装细节在
//! `storage::codec` / `storage::session` 那边）；它只负责把"哪一版"这个决定传下去，
//! 并把上层真正想知道的几件事（读得动吗、解开了吗、封了什么）问出来。

use super::{token_to_rev, ProtectionReport};
use crate::storage::codec::{self, Policy, Secrets};
use crate::storage::session;
use crate::vault::database::Database;
use crate::vault::notes::Reading;

/// 一篇笔记某一版的头（见 `Database::note_head`）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoteHead {
    pub needs_unlock: bool,
    pub needs_passphrase: bool,
    pub needs_secret_key: bool,
    pub passphrase_ready: bool,
}

impl Database {
    /// 导出：把某一版的 markdown **原文**写到用户选的位置。
    ///
    /// 只写正文，不带任何外壳 —— 导出的就是笔记里写下的那些字，拿到别处照样读得懂。
    /// 上锁的版本要先解锁（与"读它"同一条路，不另开一条能绕过口令的道）。
    pub fn export_note(
        &self,
        title: &str,
        reference: Option<&str>,
        target: &std::path::Path,
    ) -> Result<(), String> {
        let Reading::Ready { note } = self.read_note(title, reference)? else {
            return Err("这一版是加密的：先解锁，再导出".to_string());
        };
        crate::storage::workspace::write_bytes(target, note.markdown.as_bytes())
    }

    /// 导出：把某一版渲染成**一段完整的 HTML 文档**写到目标位置。
    ///
    /// 与 [`Self::export_note`]（markdown 原文）的区别有两处，都是"完整"带来的：
    ///
    /// - **自带骨架**：`render_html` 给的是正文片段（`<p>`、`<h1>` 那些），
    ///   直接存成文件、用浏览器打开就是一堆裸标签。这里补上 `<meta charset>`
    ///   （不然中文在部分浏览器里显示成乱码）、标题，以及一段把正文样式
    ///   一起带出去的 `<style>` —— **不引外部 CSS**：导出的文件要能单独拿走、
    ///   在没有网络的地方照样能看，而笔记里可能引用了图片与模板资源。
    /// - **内部链接指不出去**：仓库外的地址（`../某页`）在单个文件里没有意义，
    ///   所以渲染时**不给解析器**（`None`）—— 链接仍是链接，但不是"能点的内部链接"。
    ///   这与阅读页相反：那里能点是因为程序知道怎么跳。
    pub fn export_note_html(
        &self,
        title: &str,
        reference: Option<&str>,
        target: &std::path::Path,
    ) -> Result<(), String> {
        let Reading::Ready { note } = self.read_note(title, reference)? else {
            return Err("这一版是加密的：先解锁，再导出".to_string());
        };
        let document = crate::vault::export_html::wrap_document(title, &note.html);
        crate::storage::workspace::write_bytes(target, document.as_bytes())
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
            compression: protection.compress.then_some(protection.compression),
            cipher: protection.symmetric.then_some(protection.cipher),
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

    /// 一篇笔记某一版的**头**：怎么存的、这次会话里口令在不在。
    ///
    /// **不需要口令** —— 封装的头本来就是明文，所以"要不要解锁"这件事在读字节之前
    /// 就已经答得出来（`features::files::file_info` 那边是同一件事，文件侧另有一份）。
    ///
    /// ## 为什么不复用 `file_info`
    ///
    /// `file_info` 先过 `file_title`，它会把**不是文件**的标题塞进 `File:` 前缀 ——
    /// 于是拿它问一篇笔记会去找 `File:卡片`，找不到。所以笔记侧得自己走一遍。
    /// 两边逻辑几乎一样是刻意的：它们回答的是同一个问题（"这一份要不要解锁"），
    /// 分成两个函数是为了让"文件标题的规范化"只留在文件那一侧。
    pub fn note_head(&self, title: &str, reference: Option<&str>) -> Result<NoteHead, String> {
        let rev = match reference {
            Some(token) => Some(token_to_rev(token)?),
            None => None,
        };
        let (id, rev) = self.resolve_revision(title, rev)?;

        let state = self.state_of(&id)?;
        let blob = if rev == state.rev {
            state.blob.clone()
        } else {
            self.event_at(&id, title, rev)?.1
        };
        // 还没有正文的那一版（0 版）没什么可解的，当成没套层。
        // 所以这里不去造一个 `Protection`（那一版压根没有头），直接算三个布尔 ——
        // 空 blob 当成"什么层都没套"，和 `unlock` 里的 `sealed` 是同一个判断。
        let sealed = !blob.is_empty() && self.blobs().protection(&blob)?.symmetric;
        let encrypted = !blob.is_empty() && self.blobs().protection(&blob)?.encrypt.is_some();

        Ok(NoteHead {
            needs_unlock: sealed || encrypted,
            needs_passphrase: sealed,
            needs_secret_key: encrypted,
            passphrase_ready: session::passphrase_for(&id, rev).is_some(),
        })
    }

    /// 给某一版解锁：`reference` 是 token，`None` = 最新版。
    ///
    /// **当场验一遍**：口令对不对，只有真拿它去解一次才知道。不验的话错的口令也照存，
    /// 界面以为解开了、去读却读不出来 —— 于是"再输一次"这条路就断了，错误还会伪装成
    /// 别的东西（图片那边就表现成"图片不存在"）。
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

        let state = self.state_of(&id)?;
        let blob = if rev == state.rev {
            state.blob.clone()
        } else {
            self.event_at(&id, title, rev)?.1
        };

        // 只有**口令层**才验：gpg 那几层问的是钥匙串，与这里的口令无关；
        // 还没有正文的那一版（0 版）也没什么可解的
        let sealed = if blob.is_empty() {
            false
        } else {
            self.blobs().protection(&blob)?.symmetric
        };
        if !sealed {
            session::unlock(&id, rev, passphrase.to_string());
            return Ok(());
        }

        match self.blobs().get(
            &blob,
            &Secrets {
                passphrase: Some(passphrase),
            },
        ) {
            Ok(_) => {
                session::unlock(&id, rev, passphrase.to_string());
                Ok(())
            }
            Err(error) => {
                // 错的就别留着：留着只会让"再输一次"变成不可能
                session::forget(&id, rev);
                Err(error)
            }
        }
    }
}
