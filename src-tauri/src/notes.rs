//! 笔记：名字表、事件日志、草稿槽位。
//!
//! 三件事分得很清：
//!
//! - **名字表**（`titles.json`）：id ↔ 标题。磁盘上的文件名是生成的 ASCII 标识，
//!   原始标题只活在这张表里 —— 于是中文、空格、重名、跨系统的编码问题都不会变成
//!   文件名的一部分。改名也就只是改表里一行。
//! - **事件日志**（`objects/0/<id>.log`）：一行一条，只追加、不重写。当前状态由整条
//!   日志重放（`fold`）出来，所以"改了什么"永远有据可查。
//! - **草稿槽位**（`drafts/<id>`）：**一个文档一个槽位**，可覆盖。
//!   草稿是还没定稿的工作状态，不是历史的一版 —— 把它塞进日志或 blob 仓，
//!   等于拿不可变存储扛高频写。它不占版本、不进仓；封装照**这篇笔记当前的保护**，
//!   但**不签名**（签名留给提交）。
//!
//! 保护是**这篇笔记的属性**（照最新一版落盘时的封装），不是每次写入的参数：不给策略的
//! 提交与草稿都从它出发，不会因为某次没写参数而偷偷降级；要改就显式给一次新策略
//! （见 `commit_with`）。回滚有两种做法：照当前保护重写（`rollback`），
//! 或直接复制那一版的封装（`rollback_copy`）。

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::codec::{Meta, Policy, Protection, Secrets};
use crate::database::{now, Database, MAIN_NS};
use crate::session;
use crate::store::hash_hex;
use crate::title::LinkResolver;
use crate::workspace::{append_line, read_json, write_bytes, write_json};

const DEFAULT_MIME: &str = "text/markdown";

/// 笔记正文的自述：内容都是 markdown
fn body_meta() -> Meta {
    Meta {
        mime: DEFAULT_MIME.to_string(),
    }
}

/// 名字表
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Titles {
    /// id → 标题
    pub notes: BTreeMap<String, String>,
    /// id → 标题（已删除，文件在 trash/ 里）
    pub trashed: BTreeMap<String, String>,
}

/// 日志里的一行。任何变更都是追加一条，文件永不重写。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "lowercase")]
pub enum Event {
    /// 笔记建立
    Meta {
        at: String,
        ns: String,
        title: String,
    },
    /// 一次提交
    Rev {
        at: String,
        rev: u64,
        /// 内容落在哪个 blob 上
        blob: String,
        bytes: u64,
        mime: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        summary: Option<String>,
    },
}

/// 由整条日志重放出来的当前状态
#[derive(Debug, Clone, Default)]
pub struct NoteState {
    pub ns: String,
    pub title: String,
    /// 建立时间
    pub created: String,
    /// 最后一次提交时间
    pub modified: String,
    /// 最新一版落在哪个 blob 上（0 版没有正文）
    pub blob: String,
    pub bytes: u64,
    pub rev: u64,
    /// 最后一次提交的说明
    pub summary: Option<String>,
}

/// 一篇笔记
#[derive(Debug, Clone, Serialize)]
pub struct Note {
    /// 规范键，形如 `0:标题`
    pub key: String,
    pub title: String,
    /// 原样源码
    pub markdown: String,
    /// 阅读用的 HTML。内部链接已经标好红/蓝
    pub html: String,
    pub rev: u64,
    pub created: String,
    pub modified: String,
    pub summary: Option<String>,
    /// 这一版落盘时用了哪些层 —— **看头就知道，不需要口令**
    pub protection: Protection,
}

/// 列表里的一条
#[derive(Debug, Clone, Serialize)]
pub struct NoteSummary {
    pub key: String,
    pub title: String,
    pub rev: u64,
    pub bytes: u64,
    pub created: String,
    pub modified: String,
}

/// 历史上的一版。
///
/// **只有提交**：草稿是每篇一个可覆盖槽位，不进事件链，所以它不是这里的一行。
#[derive(Debug, Clone, Serialize)]
pub struct RevisionSummary {
    /// 版本号，从 1 开始连续
    pub rev: u64,
    pub at: String,
    pub bytes: u64,
    pub summary: Option<String>,
    /// 这一版落盘时用了哪些层 —— 各版可能不同
    pub protection: Protection,
}

/// 交给界面读的结果。
///
/// 读不到**不是**错误：这一页上了锁时要说得出"需要口令"，界面才好把人带到输入口令
/// 那一页去，而不是丢一个看不懂的错误。
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum Reading {
    /// 读到了
    Ready { note: Note },
    /// 上了锁：需要**这一页**的口令。
    ///
    /// `wrong_passphrase` 表示刚才那把是错的（这次会话里的已被丢掉）——
    /// 界面据此说"再试一次"，而不是让人对着一个没有反应的输入框发呆。
    Locked {
        protection: Protection,
        wrong_passphrase: bool,
    },
}

/// 一份草稿
#[derive(Debug, Clone, Serialize)]
pub struct Draft {
    pub markdown: String,
    pub modified: String,
    pub protection: Protection,
}

/// 把整条日志折成当前状态
pub fn fold(events: &[Event]) -> NoteState {
    let mut state = NoteState::default();

    for event in events {
        match event {
            Event::Meta { at, ns, title } => {
                state.ns = ns.clone();
                state.title = title.clone();
                state.created = at.clone();
                state.modified = at.clone();
            }
            Event::Rev {
                at,
                rev,
                blob,
                bytes,
                summary,
                ..
            } => {
                state.modified = at.clone();
                state.blob = blob.clone();
                state.bytes = *bytes;
                state.rev = *rev;
                state.summary = summary.clone();
            }
        }
    }

    state
}

impl Database {
    pub fn titles(&self) -> Result<Titles, String> {
        Ok(read_json::<Titles>(&self.titles_path()))
    }

    pub fn save_titles(&self, titles: &Titles) -> Result<(), String> {
        write_json(&self.titles_path(), titles)
    }

    /// 把地址里写的东西规整成**显示标题**。
    ///
    /// 创建、查表、地址解析都过这一道 —— 同一把尺子，所以 `example` 与 `Example`
    /// 是同一篇。
    pub fn display_of(&self, title: &str) -> Result<String, String> {
        crate::title::parse_main(title)
    }

    /// 一篇笔记的日志路径
    pub fn log_path(&self, id: &str) -> PathBuf {
        self.note_path(MAIN_NS, id)
    }

    /// 已删除笔记的日志路径
    pub fn trash_path(&self, id: &str) -> PathBuf {
        self.trash_note_path(MAIN_NS, id)
    }

    /// 标题 → id
    ///
    /// 查表前先把名字规整成显示标题：命令也可能被直接调用（前端、脚本），
    /// 与地址解析用同一把尺子才不会出现"建了 example、敲 Example 找不到"。
    pub fn id_of(&self, title: &str) -> Option<String> {
        let display = self.display_of(title).ok()?;

        self.titles()
            .ok()?
            .notes
            .iter()
            .find(|(_, name)| name.as_str() == display)
            .map(|(id, _)| id.clone())
    }

    /// 标题在不在
    pub fn exists(&self, title: &str) -> bool {
        self.id_of(title).is_some()
    }

    /// 标题 → id。找不到就是"没有这篇"。
    pub(crate) fn locate(&self, title: &str) -> Result<String, String> {
        self.id_of(title)
            .ok_or_else(|| format!("没有这篇笔记：{title}"))
    }

    fn read_events(&self, id: &str) -> Result<Vec<Event>, String> {
        let path = self.log_path(id);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            // 日志不在 = 这篇没有历史（刚建、或被人删了文件），不是错误
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(format!("读不出 {}：{error}", path.display())),
        };

        let mut events = Vec::new();
        for line in text.lines() {
            if line.trim().is_empty() {
                continue;
            }
            events.push(
                serde_json::from_str(line)
                    .map_err(|error| format!("{} 里有一行读不出来：{error}", path.display()))?,
            );
        }
        Ok(events)
    }

    fn append(&self, ns: &str, id: &str, event: &Event) -> Result<(), String> {
        let line = serde_json::to_string(event).map_err(|error| format!("写不进日志：{error}"))?;
        append_line(&self.note_path(ns, id), &line)
    }

    pub(crate) fn state_of(&self, id: &str) -> Result<NoteState, String> {
        Ok(fold(&self.read_events(id)?))
    }

    /// 建一篇笔记，返回它的**显示标题**（调用方拿它去导航）。
    ///
    /// 名字与地址解析走同一套规整与词法检查：存进去的必须是**能被解析回来**的形状，
    /// 否则就成了"建得出来、却打不开"的笔记。
    pub fn create(&self, title: &str) -> Result<String, String> {
        let display = self.display_of(title)?;
        let mut titles = self.titles()?;
        if titles.notes.values().any(|name| name == &display) {
            return Err(format!("已经有一篇叫「{display}」的笔记"));
        }

        let id = next_id(&titles);
        self.append(
            MAIN_NS,
            &id,
            &Event::Meta {
                at: now(),
                ns: MAIN_NS.to_string(),
                title: display.clone(),
            },
        )?;

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
    fn current_policy(&self, state: &NoteState) -> Result<Policy, String> {
        if state.blob.is_empty() {
            return Ok(self.protection());
        }

        let protection = self.blobs().protection(&state.blob)?;
        Ok(Policy {
            compress: protection.compress,
            gpg_sign: protection.sign,
            gpg_encrypt: protection.encrypt,
            symmetric: protection.symmetric,
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
        let id = self.locate(title)?;
        let state = self.state_of(&id)?;
        let rev = state.rev + 1;

        let policy = match protection {
            Some(policy) => policy,
            None => self.current_policy(&state)?,
        };
        let passphrase = passphrase.or_else(|| session::passphrase_for(&id, state.rev));

        let blob = self.blobs().put(
            markdown.as_bytes(),
            &body_meta(),
            &policy,
            passphrase.as_deref(),
        )?;

        // 写成了才把口令顺延给新版本：解锁一次，读写全通
        if let Some(passphrase) = passphrase {
            session::unlock(&id, rev, passphrase);
        }

        self.append(
            MAIN_NS,
            &id,
            &Event::Rev {
                at: now(),
                rev,
                blob,
                bytes: markdown.len() as u64,
                mime: DEFAULT_MIME.to_string(),
                summary,
            },
        )?;

        self.read(title)
    }

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
            MAIN_NS,
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
            Err(error) if is_wrong_passphrase(&error) => {
                // 口令不对：丢掉**这一版**的那把，这样界面把人带回去时还能重新输
                session::forget(&id, state.rev);
                Ok(Reading::Locked {
                    protection: self.blobs().protection(&state.blob)?,
                    wrong_passphrase: true,
                })
            }
            Err(error) => Err(error),
        }
    }

    /// 交给界面读**某一版**：同一套判断
    pub fn read_revision_for_display(&self, title: &str, rev: u64) -> Result<Reading, String> {
        let id = self.locate(title)?;
        let (at, blob, _, summary) = self.event_at(&id, title, rev)?;

        if let Some(protection) = self.lock_of(&id, rev, &blob)? {
            return Ok(Reading::Locked {
                protection,
                wrong_passphrase: false,
            });
        }

        let state = self.state_of(&id)?;
        // 保护头要读**这一版**的 blob：`assemble` 会把它拿走，所以先留一份
        match self.assemble(&id, &state, rev, at, blob.clone(), summary) {
            Ok(note) => Ok(Reading::Ready { note }),
            Err(error) if is_wrong_passphrase(&error) => {
                session::forget(&id, rev);
                Ok(Reading::Locked {
                    protection: self.blobs().protection(&blob)?,
                    wrong_passphrase: true,
                })
            }
            Err(error) => Err(error),
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
    ) -> Result<Note, String> {
        let display = state.title.clone();

        let (markdown, protection) = if blob.is_empty() {
            // 还没有正文（0 版），也就无所谓封装
            (
                String::new(),
                Protection {
                    compress: false,
                    sign: None,
                    encrypt: None,
                    symmetric: false,
                },
            )
        } else {
            let passphrase = session::passphrase_for(id, rev);
            let bytes = self.blobs().get(
                &blob,
                &Secrets {
                    passphrase: passphrase.as_deref(),
                },
            )?;
            let markdown =
                String::from_utf8(bytes).map_err(|_| format!("「{display}」不是文本"))?;

            (markdown, self.blobs().protection(&blob)?)
        };

        let html = self.render_html(&markdown, &display)?;

        Ok(Note {
            key: format!("{}:{}", state.ns, state.title),
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
    /// 内部链接要判红蓝，所以得先把「现有的全部页面名」交给解析器 ——
    /// 渲染发生在 markdown-it 的回调里，那时没有仓库可查。
    ///
    /// 阅读页与编辑器预览都走这里：**渲染只有一处**，所以"预览里是什么样"
    /// 与"存下来再读是什么样"不会分家。
    pub fn render_html(&self, markdown: &str, title: &str) -> Result<String, String> {
        let resolver = LinkResolver::new(self.link_keys()?, Some(title.to_string()));
        Ok(crate::markdown::render_with(markdown, Some(&resolver)))
    }

    /// 现有笔记的页面名集合（链接解析用它判红 / 蓝链）
    fn link_keys(&self) -> Result<Arc<HashSet<String>>, String> {
        Ok(Arc::new(self.titles()?.notes.values().cloned().collect()))
    }

    /// 这一篇的全部提交，**新的在前**。
    ///
    /// 每一版都带上它自己的存储状态，所以版本列表里就能看出哪几版是加密的。
    pub fn revisions_of(&self, title: &str) -> Result<Vec<RevisionSummary>, String> {
        let id = self
            .id_of(title)
            .ok_or_else(|| format!("没有这篇笔记：{title}"))?;

        let mut out = Vec::new();
        for event in self.read_events(&id)? {
            let Event::Rev {
                at,
                rev,
                blob,
                bytes,
                summary,
                ..
            } = event
            else {
                // `Meta` 只说"这篇建立过"，不是一版
                continue;
            };

            out.push(RevisionSummary {
                rev,
                at,
                bytes,
                summary,
                protection: self.blobs().protection(&blob)?,
            });
        }

        out.reverse();
        Ok(out)
    }

    /// 删除一篇笔记。
    ///
    /// **不是抹掉**：日志挪进 `trash/`，名字从 `notes` 挪到 `trashed` —— 所以还捞得
    /// 回来；blob 一个都不动，回收交给 GC。
    pub fn delete(&self, title: &str) -> Result<(), String> {
        let id = self.locate(title)?;
        let display = self.display_of(title)?;

        let from = self.log_path(&id);
        if from.is_file() {
            let to = self.trash_path(&id);
            fs::rename(&from, &to)
                .map_err(|error| format!("挪不动 {}：{error}", from.display()))?;
        }

        // 草稿跟着走。留一个没有主的槽位，下次建同名笔记会莫名其妙"有草稿"。
        match fs::remove_file(self.draft_path(&id)) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("删不掉草稿：{error}")),
        }

        let mut titles = self.titles()?;
        titles.notes.remove(&id);
        titles.trashed.insert(id, display);
        self.save_titles(&titles)
    }

    /// 列出全部笔记。
    ///
    /// 创建与修改时间**从日志推出来**，不另外存一份 —— 那样才有两个真相来源。
    pub fn list(&self) -> Result<Vec<NoteSummary>, String> {
        let titles = self.titles()?;
        let mut out = Vec::new();

        for (id, display) in &titles.notes {
            let state = self.state_of(id)?;
            out.push(NoteSummary {
                key: format!("{}:{}", state.ns, state.title),
                title: display.clone(),
                rev: state.rev,
                bytes: state.bytes,
                created: state.created,
                modified: state.modified,
            });
        }

        // 最近改过的排前面
        out.sort_by(|a, b| b.modified.cmp(&a.modified));
        Ok(out)
    }

    // ------------------------------------------------------------ 草稿槽位

    /// 存草稿。**一个文档一个槽位**，直接覆盖。
    ///
    /// 封装照**这篇笔记当前的保护**，但**不签名**（签名留给提交）；也不占版本、不进 blob 仓 ——
    /// 草稿是高频写，进仓就变成"每三秒产生一个不可变对象"。
    pub fn save_draft(&self, title: &str, markdown: &str) -> Result<(), String> {
        let id = self
            .id_of(title)
            .ok_or_else(|| format!("没有这篇笔记：{title}"))?;
        let passphrase = session::latest_for(&id);

        // 草稿的**加密**不低于这篇笔记当前的保护（不然那些层就白做了）；签名除外 ——
        // 签名是提交的出处证明，草稿只是本地工作态，自动保存每次动 gpg
        //（还可能弹 pinentry）不值当。
        let mut policy = self.current_policy(&self.state_of(&id)?)?;
        policy.gpg_sign = None;

        let file = crate::codec::encode(
            markdown.as_bytes(),
            &body_meta(),
            &policy,
            passphrase.as_deref(),
        )
        .map_err(|error| error.to_string())?;

        write_bytes(&self.draft_path(&id), &file)
    }

    /// 读草稿。没有就是没有（不是错误）。
    pub fn load_draft(&self, title: &str) -> Result<Option<Draft>, String> {
        let id = self
            .id_of(title)
            .ok_or_else(|| format!("没有这篇笔记：{title}"))?;
        let path = self.draft_path(&id);

        let file = match fs::read(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("读不出 {}：{error}", path.display())),
        };

        let protection = crate::codec::inspect(&file)
            .map_err(|error| error.to_string())?
            .protection;
        let passphrase = session::latest_for(&id);
        let bytes = crate::codec::decode(
            &file,
            &Secrets {
                passphrase: passphrase.as_deref(),
            },
        )
        .map_err(|error| error.to_string())?;

        let markdown =
            String::from_utf8(bytes).map_err(|_| format!("「{title}」的草稿不是文本"))?;

        Ok(Some(Draft {
            markdown,
            modified: modified_at(&path),
            protection,
        }))
    }

    /// 丢掉草稿
    pub fn discard_draft(&self, title: &str) -> Result<bool, String> {
        let id = self
            .id_of(title)
            .ok_or_else(|| format!("没有这篇笔记：{title}"))?;

        match fs::remove_file(self.draft_path(&id)) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(format!("删不掉草稿：{error}")),
        }
    }
}

/// 这个错是不是"口令不对"。
///
/// 认的是那句话本身，而它的写法只有 `codec` 一处 —— 两边不会各自漂走。
fn is_wrong_passphrase(error: &str) -> bool {
    error.contains(crate::codec::WRONG_PASSPHRASE_MESSAGE)
}

/// 生成一个没被用过的 id：纯 ASCII 十六进制，撞了就再取一个
fn next_id(titles: &Titles) -> String {
    let mut salt = 0u64;
    loop {
        let id: String = hash_hex(format!("{}:{salt}", now()).as_bytes())
            .chars()
            .take(16)
            .collect();
        if !titles.notes.contains_key(&id) && !titles.trashed.contains_key(&id) {
            return id;
        }
        salt += 1;
    }
}

/// 文件的修改时间（RFC3339）。取不到就给空串 —— 它只是给人看的。
fn modified_at(path: &Path) -> String {
    let Ok(modified) = fs::metadata(path).and_then(|meta| meta.modified()) else {
        return String::new();
    };
    let Ok(stamp) = modified.duration_since(std::time::UNIX_EPOCH) else {
        return String::new();
    };
    let Ok(moment) = time::OffsetDateTime::from_unix_timestamp(stamp.as_secs() as i64) else {
        return String::new();
    };
    moment
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::Policy;
    use crate::database::{Config, Database};

    fn scratch(name: &str) -> Database {
        let dir = std::env::temp_dir().join(format!(
            "refind-note-notes-test-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        let workspace = crate::workspace::Workspace::open(dir).unwrap();
        Database::open(&workspace).unwrap()
    }

    fn cleanup(database: &Database) {
        // 库在 `<暂存目录>/db` 下，settings 是它的兄弟目录 —— 从暂存根整棵删掉
        if let Some(root) = database.root().parent() {
            let _ = fs::remove_dir_all(root);
        }
    }

    /// 口令是**进程内共享**的（`session`），几个用例同时跑会互相踩。
    /// 碰它的用例先拿这把锁。
    static SESSION_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn session_guard() -> std::sync::MutexGuard<'static, ()> {
        SESSION_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// 数一数字节仓里有几个 blob。
    ///
    /// 只为"草稿不进仓"那条用例 —— 生产代码里不必为了测试留一个遍历接口，
    /// 那个等 GC 的时候再写。
    fn blob_count(database: &Database) -> usize {
        let root = database.root().join("blobs");
        let mut count = 0;
        let Ok(shards) = fs::read_dir(&root) else {
            return 0;
        };
        for shard in shards.flatten() {
            let Ok(files) = fs::read_dir(shard.path()) else {
                continue;
            };
            for file in files.flatten() {
                if !file.file_name().to_string_lossy().ends_with(".tmp") {
                    count += 1;
                }
            }
        }
        count
    }

    #[test]
    fn a_note_is_created_committed_and_read_back() {
        let _guard = session_guard();
        let database = scratch("roundtrip");
        session::forget_all();

        database.create("示例笔记").unwrap();
        assert!(database.exists("示例笔记"));

        let note = database
            .commit("示例笔记", "# 标题\n\n正文", Some("第一次".to_string()))
            .unwrap();
        assert_eq!(note.title, "示例笔记");
        assert_eq!(note.rev, 1);
        assert_eq!(note.markdown, "# 标题\n\n正文");
        assert_eq!(note.summary.as_deref(), Some("第一次"));
        assert!(!note.created.is_empty() && !note.modified.is_empty());

        // 再读一遍走的是日志 → blob 这条路
        let again = database.read("示例笔记").unwrap();
        assert_eq!(again.markdown, note.markdown);
        assert_eq!(again.created, note.created);

        // 提交两次就是两版
        database
            .commit("示例笔记", "# 标题\n\n改过了", None)
            .unwrap();
        let latest = database.read("示例笔记").unwrap();
        assert_eq!(latest.rev, 2);
        assert_eq!(latest.markdown, "# 标题\n\n改过了");

        cleanup(&database);
    }

    #[test]
    fn titles_are_unique_and_errors_are_explained() {
        let database = scratch("unique");

        database.create("甲").unwrap();
        let error = database.create("甲").unwrap_err();
        assert!(error.contains("已经有一篇"), "{error}");

        assert!(database.create("   ").is_err());
        assert!(database.read("乙").unwrap_err().contains("没有这篇"));

        cleanup(&database);
    }

    #[test]
    fn a_created_title_is_stored_in_the_shape_the_parser_looks_up() {
        let _guard = session_guard();
        let database = scratch("canonical");
        session::forget_all();

        // 建的时候写小写、下划线，存下来必须是规范形状 ——
        // 否则"建了 example_note、地址栏敲 Example note"会找不到自己
        let created = database.create("example_note").unwrap();
        assert_eq!(created, "Example note");
        assert!(database.exists("Example note"));
        // 换个写法也找得到：地址解析与这里是同一把尺子
        assert!(database.exists("example_note"));

        // 名字本身不合法的建不出来 —— 否则会造出一篇打不开的笔记
        assert!(database.create("带<尖括号>").is_err());
        // 冒号前缀现在都不是可存储的命名空间
        assert!(database.create("foo:bar").is_err());
        assert!(database.create("special:all").is_err());

        cleanup(&database);
    }

    #[test]
    fn the_layout_lands_on_the_first_open() {
        let _guard = session_guard();
        let database = scratch("layout");
        session::forget_all();

        // 该在的东西第一次打开就该在：主命名空间的日志目录与回收站目录
        assert!(database
            .root()
            .join("objects")
            .join(crate::database::MAIN_NS)
            .is_dir());
        assert!(database
            .root()
            .join("trash")
            .join(crate::database::MAIN_NS)
            .is_dir());

        cleanup(&database);
    }

    #[test]
    fn a_created_note_can_always_be_reached_through_the_address_bar() {
        let _guard = session_guard();
        let database = scratch("openable");
        session::forget_all();

        // 用非规范写法建：小写、下划线
        let created = database.create("my_new_note").unwrap();
        assert_eq!(created, "My new note");

        // 关键一条：建完必须能被**地址解析**找回来 —— 创建与解析过的是同一把尺子，
        // 所以人怎么写都行，不会出现"建得出来、却永远打不开"的笔记。
        for input in [
            "my_new_note",
            "My new note",
            "my new note",
            "  My   new  note  ",
        ] {
            let parsed = crate::address::parse(input)
                .unwrap_or_else(|reason| panic!("{input:?} 本该解析成功：{reason}"))
                .unwrap_or_else(|| panic!("{input:?} 不是空输入，应当是一个地址"));

            assert_eq!(
                parsed.address.page, created,
                "{input:?} 解析出来的页面名应当就是刚建的那一篇"
            );
            assert!(database.exists(&parsed.address.page));
        }

        cleanup(&database);
    }

    #[test]
    fn the_list_carries_times_derived_from_the_log() {
        let _guard = session_guard();
        let database = scratch("list");
        session::forget_all();

        database.create("甲").unwrap();
        database.create("乙").unwrap();
        database.commit("甲", "内容", None).unwrap();

        let listed = database.list().unwrap();
        assert_eq!(listed.len(), 2);

        let first = listed.iter().find(|item| item.title == "甲").unwrap();
        assert_eq!(first.rev, 1);
        assert!(!first.created.is_empty() && !first.modified.is_empty());
        assert_eq!(first.key, "0:甲");

        cleanup(&database);
    }

    #[test]
    fn a_draft_lives_in_its_own_slot_and_never_enters_the_blob_store() {
        let _guard = session_guard();
        let database = scratch("draft");
        session::forget_all();

        database.create("甲").unwrap();
        database.commit("甲", "提交过的内容", None).unwrap();
        let blobs_before = blob_count(&database);

        database.save_draft("甲", "还没定稿").unwrap();
        database.save_draft("甲", "又改了一版").unwrap();

        // 草稿可覆盖，且一个 blob 都不多
        assert_eq!(blob_count(&database), blobs_before);

        let draft = database.load_draft("甲").unwrap().unwrap();
        assert_eq!(draft.markdown, "又改了一版");
        // 封装照这篇当前的保护（新笔记 = 仓库默认），所以是压缩
        assert!(draft.protection.compress);

        // 草稿不影响正文
        assert_eq!(database.read("甲").unwrap().markdown, "提交过的内容");

        assert!(database.discard_draft("甲").unwrap());
        assert!(database.load_draft("甲").unwrap().is_none());
        assert!(!database.discard_draft("甲").unwrap());

        cleanup(&database);
    }

    #[test]
    fn a_draft_goes_through_the_same_layers_as_a_commit() {
        let _guard = session_guard();
        let database = scratch("draft-sealed");
        session::forget_all();

        // 仓库默认是"压缩 + 口令对称"（新笔记从它出发）
        database
            .save_config(&Config {
                protection: Policy {
                    compress: true,
                    symmetric: true,
                    ..Default::default()
                },
            })
            .unwrap();
        database.create("甲").unwrap();

        // 没给口令：写不进去，说得清是缺口令
        let error = database.save_draft("甲", "秘密").unwrap_err();
        assert!(error.contains("口令"), "{error}");

        // 口令按页存，所以要给**这一篇**解锁
        let id = database.id_of("甲").unwrap();
        session::unlock(&id, 1, "pw".to_string());
        database.save_draft("甲", "秘密").unwrap();

        // 槽位里翻不到原文，但头是明文 —— 不输口令也知道它是加密的
        let raw = fs::read(database.draft_path(&id)).unwrap();
        assert!(!raw.windows(6).any(|window| window == "秘密".as_bytes()));

        let draft = database.load_draft("甲").unwrap().unwrap();
        assert!(draft.protection.symmetric && draft.protection.compress);
        assert_eq!(draft.markdown, "秘密");

        // 上了锁就读不出来
        session::forget_all();
        assert!(database.load_draft("甲").is_err());

        cleanup(&database);
    }

    #[test]
    fn a_commit_inherits_the_notes_current_protection() {
        let _guard = session_guard();
        let database = scratch("protection");
        session::forget_all();

        database.create("甲").unwrap();

        // 显式给一次 = 换保护：这一版起用原样（仓库默认本来是压缩）
        let plain = database
            .commit_with("甲", "明文", None, Some(Policy::default()), None)
            .unwrap();
        assert!(plain.protection.is_plain());

        // 之后不给策略的提交**照这篇当前的保护**，不会悄悄换回仓库默认
        let again = database.commit("甲", "还是明文", None).unwrap();
        assert!(again.protection.is_plain(), "不给策略不会悄悄换封装");

        // 再显式换一次：从这一版起照新的粘住
        let packed = database
            .commit_with(
                "甲",
                "压过的内容，长一点好看出效果",
                None,
                Some(Policy {
                    compress: true,
                    ..Default::default()
                }),
                None,
            )
            .unwrap();
        assert!(packed.protection.compress);
        assert!(database.read("甲").unwrap().protection.compress);

        cleanup(&database);
    }

    #[test]
    fn a_locked_page_says_so_instead_of_failing() {
        let _guard = session_guard();
        let database = scratch("locked");
        session::forget_all();

        database
            .save_config(&Config {
                protection: Policy {
                    symmetric: true,
                    ..Default::default()
                },
            })
            .unwrap();
        database.create("甲").unwrap();

        let id = database.id_of("甲").unwrap();
        // 口令按**版本**存，所以提交时把它记给新写下的那一版
        database
            .commit_with("甲", "秘密内容", None, None, Some("pw".to_string()))
            .unwrap();

        // 上锁之后再读：**不是报错**，而是明说"这一页需要口令"
        session::forget_all();
        assert!(matches!(
            database.read_for_display("甲").unwrap(),
            Reading::Locked { protection, .. } if protection.symmetric
        ));

        // 给这一版解锁就正常了
        session::unlock(&id, 1, "pw".to_string());
        assert!(matches!(
            database.read_for_display("甲").unwrap(),
            Reading::Ready { note } if note.markdown == "秘密内容"
        ));

        // 口令按页存：给别的页解锁，不等于这一页能读
        session::forget_all();
        session::unlock("别的页的标识", 1, "pw".to_string());
        assert!(matches!(
            database.read_for_display("甲").unwrap(),
            Reading::Locked { .. }
        ));

        cleanup(&database);
    }

    #[test]
    fn a_wrong_passphrase_can_be_tried_again() {
        let _guard = session_guard();
        let database = scratch("wrong-passphrase");
        session::forget_all();

        database
            .save_config(&Config {
                protection: Policy {
                    symmetric: true,
                    ..Default::default()
                },
            })
            .unwrap();
        database.create("甲").unwrap();

        let id = database.id_of("甲").unwrap();
        database
            .commit_with("甲", "秘密内容", None, None, Some("right".to_string()))
            .unwrap();
        session::forget(&id, 1);

        // 输错：要说得出"这一次是错的"，否则界面没法让人重输
        session::unlock(&id, 1, "wrong".to_string());
        let reading = database.read_for_display("甲").unwrap();
        assert!(
            matches!(
                reading,
                Reading::Locked {
                    wrong_passphrase: true,
                    ..
                }
            ),
            "{reading:?}"
        );

        // 错的已经丢掉 —— 所以"再输一次"这件事才成立
        assert!(session::passphrase_for(&id, 1).is_none());

        // 输对就正常
        session::unlock(&id, 1, "right".to_string());
        assert!(matches!(
            database.read_for_display("甲").unwrap(),
            Reading::Ready { note } if note.markdown == "秘密内容"
        ));

        cleanup(&database);
    }

    #[test]
    fn a_draft_is_never_signed_even_when_the_policy_says_so() {
        let _guard = session_guard();
        let database = scratch("draft-unsigned");
        session::forget_all();

        // 策略里的签名只对提交有意义。这里给一把**不存在**的密钥：草稿要是真去签，
        // 保存就会失败在"找不到签名密钥"上 —— 存得下来才是对的。
        database
            .save_config(&Config {
                protection: Policy {
                    compress: true,
                    gpg_sign: Some("不存在@example".to_string()),
                    ..Default::default()
                },
            })
            .unwrap();
        database.create("甲").unwrap();

        database.save_draft("甲", "草稿内容").unwrap();
        let draft = database.load_draft("甲").unwrap().unwrap();
        assert_eq!(draft.markdown, "草稿内容");
        assert!(draft.protection.compress, "压缩照旧");
        assert!(draft.protection.sign.is_none(), "草稿不该签名");

        cleanup(&database);
    }

    #[test]
    fn a_locked_revision_reports_its_own_protection() {
        let _guard = session_guard();
        let database = scratch("revision-locked");
        session::forget_all();

        // 第 1 版加密、第 2 版明文：两版的保护头不一样
        database
            .save_config(&Config {
                protection: Policy {
                    symmetric: true,
                    ..Default::default()
                },
            })
            .unwrap();
        database.create("甲").unwrap();
        let id = database.id_of("甲").unwrap();
        database
            .commit_with("甲", "秘密", None, None, Some("pw".to_string()))
            .unwrap();
        // 显式换回明文（不给策略的话会照第 1 版继续加密）
        database
            .commit_with("甲", "明文", None, Some(Policy::default()), None)
            .unwrap();

        // 给第 1 版一把**错**的口令：报出来的保护头必须是第 1 版自己的（加密），
        // 而不是最新一版那份（明文）
        session::forget_all();
        session::unlock(&id, 1, "wrong".to_string());
        match database.read_revision_for_display("甲", 1).unwrap() {
            Reading::Locked {
                protection,
                wrong_passphrase,
            } => {
                assert!(protection.symmetric, "保护头该来自第 1 版（加密的那版）");
                assert!(wrong_passphrase);
            }
            other => panic!("口令错了应当报上锁，而不是 {other:?}"),
        }

        cleanup(&database);
    }

    #[test]
    fn a_reencoded_rollback_keeps_the_notes_current_protection() {
        let _guard = session_guard();
        let database = scratch("rollback-reencode");
        session::forget_all();

        database.create("甲").unwrap();
        database
            .commit_with("甲", "第一版", None, Some(Policy::default()), None)
            .unwrap(); // 第 1 版：明文
        database
            .commit_with(
                "甲",
                "第二版",
                None,
                Some(Policy {
                    compress: true,
                    ..Default::default()
                }),
                None,
            )
            .unwrap(); // 第 2 版：显式换成压缩 —— 这篇当前是压缩

        let after = database
            .rollback("甲", 1, Some("回到第一版".to_string()))
            .unwrap();

        // 内容回到第一版，但封装照**当前**的保护（压缩），不是第一版那份明文
        assert_eq!(after.rev, 3);
        assert_eq!(after.markdown, "第一版");
        assert!(after.protection.compress, "重编码的回滚保持这篇当前的保护");

        cleanup(&database);
    }

    #[test]
    fn a_copy_rollback_reuses_the_blob_without_unlocking() {
        let _guard = session_guard();
        let database = scratch("rollback-copy");
        session::forget_all();

        database
            .save_config(&Config {
                protection: Policy {
                    symmetric: true,
                    ..Default::default()
                },
            })
            .unwrap();
        database.create("甲").unwrap();
        let id = database.id_of("甲").unwrap();
        database
            .commit_with("甲", "第一版", None, None, Some("pw".to_string()))
            .unwrap();
        database.commit("甲", "第二版", None).unwrap(); // 照当前保护：继续加密

        // 不解锁也能复制回滚：不是读内容，只是把第 1 版那份封装再指一次
        session::forget_all();
        let new_rev = database
            .rollback_copy("甲", 1, Some("回到第一版".to_string()))
            .unwrap();
        assert_eq!(new_rev, 3);

        // 新头是加密的（照第 1 版那份），解开之后正是第一版的内容
        assert!(matches!(
            database.read_for_display("甲").unwrap(),
            Reading::Locked { protection, .. } if protection.symmetric
        ));
        session::unlock(&id, 3, "pw".to_string());
        assert_eq!(database.read("甲").unwrap().markdown, "第一版");

        cleanup(&database);
    }

    #[test]
    fn rolling_back_writes_a_new_commit_and_keeps_the_old_ones() {
        let _guard = session_guard();
        let database = scratch("rollback");
        session::forget_all();

        database.create("甲").unwrap();
        database.commit("甲", "第一版", None).unwrap();
        database.commit("甲", "第二版", None).unwrap();

        let after = database
            .rollback("甲", 1, Some("回到第一版".to_string()))
            .unwrap();

        // 回退是**新提交**，不是把历史砍回去
        assert_eq!(after.rev, 3);
        assert_eq!(after.markdown, "第一版");
        assert_eq!(after.summary.as_deref(), Some("回到第一版"));

        // 旧记录一条都没动，所以回退本身也能再被回退
        let history = database.revisions_of("甲").unwrap();
        assert_eq!(history.len(), 3);
        assert_eq!(database.read_revision("甲", 2).unwrap().markdown, "第二版");

        // 回退到不存在的版本要报出来
        assert!(database
            .rollback("甲", 9, None)
            .unwrap_err()
            .contains("没有第 9 版"));

        cleanup(&database);
    }

    #[test]
    fn a_single_revision_can_be_read_back() {
        let _guard = session_guard();
        let database = scratch("revision");
        session::forget_all();

        database.create("甲").unwrap();
        database.commit("甲", "第一版", None).unwrap();
        database.commit("甲", "第二版", None).unwrap();

        let first = database.read_revision("甲", 1).unwrap();
        assert_eq!(first.markdown, "第一版");
        assert_eq!(first.rev, 1);
        // 建立时间来自日志里的 Meta，看哪一版都一样
        assert_eq!(first.created, database.read("甲").unwrap().created);

        // 取不存在的版本要报出来，而不是给一份空的
        assert!(database
            .read_revision("甲", 9)
            .unwrap_err()
            .contains("没有第 9 版"));

        cleanup(&database);
    }

    #[test]
    fn each_revision_reports_its_own_protection() {
        let _guard = session_guard();
        let database = scratch("revision-protection");
        session::forget_all();

        database.create("甲").unwrap();

        // 第一版明确用原样；第二版**显式换回压缩** —— 换保护是显式动作，两版封装因此不同
        database
            .commit_with("甲", "明文", None, Some(Policy::default()), None)
            .unwrap();
        database
            .commit_with(
                "甲",
                "压过的内容，写长一点好看出差别",
                None,
                Some(Policy {
                    compress: true,
                    ..Default::default()
                }),
                None,
            )
            .unwrap();

        // 各认各的：状态读的是**那一版自己的** blob 头
        assert!(database
            .read_revision("甲", 1)
            .unwrap()
            .protection
            .is_plain());
        assert!(database.read_revision("甲", 2).unwrap().protection.compress);

        cleanup(&database);
    }

    #[test]
    fn the_history_lists_commits_only_and_newest_first() {
        let _guard = session_guard();
        let database = scratch("history");
        session::forget_all();

        database.create("甲").unwrap();
        database
            .commit("甲", "第一版", Some("开头".to_string()))
            .unwrap();
        database.commit("甲", "第二版", None).unwrap();
        // 草稿是槽位，不是一版 —— 它不该出现在历史里
        database.save_draft("甲", "还没定稿").unwrap();

        let history = database.revisions_of("甲").unwrap();
        assert_eq!(history.len(), 2, "草稿不该算一版：{history:?}");
        assert_eq!(history[0].rev, 2, "新的在前");
        assert_eq!(history[1].rev, 1);
        assert_eq!(history[1].summary.as_deref(), Some("开头"));
        assert!(history[0].at >= history[1].at);

        cleanup(&database);
    }

    #[test]
    fn deleting_a_note_keeps_its_log() {
        let _guard = session_guard();
        let database = scratch("delete");
        session::forget_all();

        database.create("甲").unwrap();
        database.commit("甲", "内容", None).unwrap();
        database.save_draft("甲", "草稿").unwrap();

        let id = database.id_of("甲").unwrap();
        let log = database.log_path(&id);
        assert!(log.is_file());

        database.delete("甲").unwrap();

        // 列表里没了，也读不到了
        assert!(database.list().unwrap().is_empty());
        assert!(database.read("甲").is_err());
        assert!(database.id_of("甲").is_none());

        // 但日志还在 trash 里 —— 删错了捞得回来；草稿槽位跟着走
        assert!(!log.is_file());
        assert!(database.trash_path(&id).is_file());
        assert!(!database.draft_path(&id).exists());

        // 再删一次要报"没有这篇"
        assert!(database.delete("甲").unwrap_err().contains("没有这篇"));

        cleanup(&database);
    }
}
