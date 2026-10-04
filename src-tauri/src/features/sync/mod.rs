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

//! 与 S3 兼容服务同步：把整份仓库搬到云端，或从云端拿回来。
//!
//! 这个文件是**编排**：`run` 一趟同步（抢锁、对账、动手、放锁），`reconcile` 逐条比并上传
//! 下载，其余小函数都是它俩的零件。真正的"道理"在兄弟文件里，各管一段：
//!
//! - [`rules`]：**判定**。一个路径该怎么办，纯函数，不碰网络也不碰磁盘。全部"宁可多传一份，
//!   也别删人东西"的规矩都在那儿，测试能一条条把分支试遍。
//! - [`settings`]：**设置与账本**。同步自己的设置（这台机器的，里面有钥匙）与"上次对齐时
//!   每一份是什么样"的索引。云端那份**账本**是同一种格式，由本文件读写。
//! - [`cipher`]：**云端那一层**。传上去的字节要不要封、用哪一档算法，以及报告与进度。
//! - [`lock`]：**云端那把锁**。动手之前先抢一把，别的机器等着。
//!
//! 分层就是按这个意思切的：判定（rules）不认网络与文件系统，云端那一层（cipher）不认路径
//! 该不该同步，锁（lock）不认文件内容。哪一层要改哪条规矩，就只动那一个文件。
//!
//! ## 传什么、不传什么
//!
//! 传的是**仓库**：`db/**`（内容块、事件日志、那几张表）与 `settings/repository.json`
//! （仓库自己的设置 —— 换台机器读同一份仓库，行为要一样）。
//!
//! 不传的是**这台机器自己的东西**：`settings/preferences.json`（界面偏好）、
//! `settings/browsing.jsonl`（浏览历史）、`db/drafts/**`（写了一半的草稿槽位）、
//! 以及同步自己的设置与索引（里面还有 S3 的密钥，当然不能往上传）。
//!
//! 这个判断由**工作目录**那一层回答（见 [`is_synced`] 与它指向的函数）：工作目录长什么样
//! 是它的事，同步只管照办。
//!
//! ## 怎么定谁新
//!
//! 每个文件在 `settings/sync-index.json` 里记一份"上次对齐时的样子"（大小、时间、
//! 云端的 ETag）。于是三种情形分得清：
//!
//! - 只有本机变了 → 上传；
//! - 只有云端变了 → 下载；
//! - 两边都变了 → **按修改时间取新的那一版**，并在报告里说一声（不静默地丢东西）。
//!
//! 内容块（`db/blobs/…`）是**内容寻址**的：文件名就是内容的哈希，所以它们只会
//! "这边有那边没有"，不存在冲突 —— 这一条省掉了一大半麻烦。
//!
//! 具体的判定在 [`rules::decide`]，那才是这件事的核心；本文件只负责照它的结论动手。
//!
//! ## 删除
//!
//! 本机删掉的（上次同步时还在、现在没了）会在云端也删掉；反过来，云端没了而本机
//! 没动过的那一份，本机也删。**两边都没动过的，谁也删不动谁** —— 第一原则是别丢东西。
//!
//! ## 什么时候跑
//!
//! **要在打开数据库之前跑**（启动时那一次尤其）：新机器上 `db/titles.json`、
//! `db/namespaces.json` 这些表是打开数据库时**当场建出来的空表** —— 先开库再同步，
//! 空表会被当成"本机改过、而且更新"，把云端那份真的盖掉。所以界面那边是
//! 先同步、再打开工作目录（见 `core/preferences.ts` 里的顺序说明）。
//!
//! ## 同时只让一台机器动
//!
//! 动手之前先在云端抢一把锁（`PutObject` 带 `If-None-Match: *`，这是 S3 原生的
//! 原子操作）。锁里写着机器名与时间；超过 [`LOCK_TTL`] 没续的算过期，别人可以抢 ——
//! 机器崩了不该把同步永久锁死。这一层的细节在 [`lock`]。

mod cipher;
mod lock;
mod rules;
mod settings;

#[cfg(test)]
mod end_to_end;
#[cfg(test)]
mod tests;

use std::cell::Cell;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use time::OffsetDateTime;

use crate::storage::s3::S3;
use crate::storage::workspace::{read_json, write_json, Workspace};

pub use cipher::{CloudCipher, Conflict, Plaintext, Progress, SyncReport, SyncTransform};
pub use settings::{
    generate_key, index_facts, save_settings, settings, IndexFacts, SyncSettings,
    SyncSettingsPatch, SyncSettingsView,
};

use lock::acquire;
use rules::{decide, CloudSays, Decision, Evidence, Local, Remote};
use settings::{index_path, Index};

/// 锁多久没动静算过期（秒）
const LOCK_TTL: i64 = 300;

/// 干活期间每隔这么久续一次锁。
///
/// [`LOCK_TTL`] 的意思是"**多久没动静**就当它死了"，不是"一趟最多干多久"。
/// 不续的话，一趟超过 5 分钟的大同步会被别的机器当成过期抢走 —— 那正是这把锁要防
/// 的事。所以每 [`LOCK_REFRESH`] 就把时间戳写成现在（见 [`lock::Lock::touch`]）。
const LOCK_REFRESH: Duration = Duration::from_secs(60);

/// 每走这么多步就把索引落一次盘。
///
/// 索引是"哪些文件已经对齐"的记账本：一直攒到最后才写的话，中途被打断
/// （关窗前那颗"不等了"、断电、进程被杀）就得**从头再对一遍** —— 东西不会坏，
/// 但白传的那些要重传。隔一段写一次，中断最多让你重做这一段的活。
const INDEX_SAVE_EVERY: usize = 25;

/// 锁放在云端哪个键上（在配置的前缀之下）
pub(super) const LOCK_KEY: &str = ".sync-lock.json";

/// 云端那份**账本**：上一次同步跑完时的索引副本（在配置的前缀之下）。
///
/// 本地那份记的是"这台机器上次对齐时什么样"，它答不了两件事：
///
/// 1. **这台机器是新装的**（本地没有账）—— 那就照云端这份认路，别把云端已有的每一份
///    都当成"本机新写的"再传一遍（"新仓库盖掉云端"那个老毛病，根子也在这儿）；
/// 2. **清单里没有某一份**时，它到底是"被别的机器删了"，还是"桶被清空/清单没列全"。
///    本地账答不了，云端账能：两处都说没有，才是真删了。
///
/// 判"删没删"这件事上，**宁可多传一份，也别删人东西** —— 这一份就是那条底线的凭据。
pub(super) const LATEST_KEY: &str = ".sync-latest.json";

/// 同步跑起来的入口：抢锁、对账、动手、放锁。
pub fn run(
    workspace: &Workspace,
    settings: &SyncSettings,
    progress: &dyn Fn(Progress),
    force: bool,
) -> Result<SyncReport, String> {
    if !settings.is_ready() {
        return Err("同步没开着，或者 S3 的配置还没填全".to_string());
    }
    // 加密开着、钥匙也在：传上去的每一份都过一道；否则原样
    let transform: Box<dyn SyncTransform> = if settings.encrypt {
        Box::new(settings.cipher()?)
    } else {
        Box::new(Plaintext)
    };
    let s3 = S3::new(settings.s3.clone())?;
    let root = workspace.root().to_path_buf();
    let mut index = read_json::<Index>(&index_path(workspace));

    // ---- 抢锁 ----
    progress(Progress {
        phase: "lock".to_string(),
        done: 0,
        total: 1,
        text: if force {
            "正在抢云端同步锁…".to_string()
        } else {
            "正在取得云端同步锁…".to_string()
        },
    });
    let lock = acquire(&s3, force)?;

    // ---- 云端那份账本 ----
    //
    // 本地这份记的是"**这台机器**上次对齐时什么样"。它要是空的（新装的机器、
    // 换过桶、被清理过），而云端有一份，那就照云端的认路：不然云端已有的每一份
    // 都会被当成"本机新写的"再传一遍 —— 那条路上出过"新仓库盖掉云端"的事。
    let cloud = fetch_latest(&s3);
    if index.files.is_empty() {
        if let Some(cloud) = &cloud {
            if !cloud.files.is_empty() {
                progress(Progress {
                    phase: "lock".to_string(),
                    done: 0,
                    total: 1,
                    text: "本机没有账本，照云端的认路…".to_string(),
                });
                index = cloud.clone();
            }
        }
    }

    // 干活期间续锁 —— 顺便搭在进度回调上，不必另开线程（见 [`LOCK_REFRESH`]）
    let last_touch = Cell::new(Instant::now());
    let beating = |step: Progress| {
        if last_touch.get().elapsed() >= LOCK_REFRESH {
            last_touch.set(Instant::now());
            let _ = lock.touch(&s3);
        }
        progress(step);
    };

    let outcome = reconcile(
        &s3,
        &root,
        workspace,
        &mut index,
        Cloud {
            account: cloud.as_ref(),
            transform: transform.as_ref(),
            force_upload: settings.reupload,
            progress: &beating,
        },
    );

    // 不管成没成，锁都要放掉：留着它，别的机器要等过期才能动
    let _ = lock.release(&s3);

    let report = outcome?;
    index.save(workspace)?;

    // 云端那份账本跟着更新：它记的是"跑完这一趟之后，哪些已经对齐"。
    // 写不上去不算这一趟失败（东西都传完了），只记一笔 —— 账本旧了是**少删**，
    // 不是乱删，方向是安全的。
    if let Err(error) = publish_latest(&s3, &index) {
        eprintln!("[sync] 云端账本没写上去：{error}");
    }

    // "整份重传"是**一次性**的：这一趟跑完就清掉，免得此后每次都重传
    if settings.reupload {
        let mut updated = settings.clone();
        updated.reupload = false;
        save_settings(workspace, &updated)?;
    }
    Ok(report)
}

/// 对账要用的"云端侧那几样"，捆成一个结构传。
///
/// 原来是四个平行的参数（云端账本 / 变换 / 整份重传 / 进度回调），加上 s3、root、
/// workspace、index 就是八个 —— 调用处那一串 `&beating` 后面跟谁，全靠回头数。
/// 捆起来之后参数表只说"在哪、拿哪份账、怎么封、报给谁"四件事，
/// 而 [`reconcile`] 的签名也回到了能一眼看完的长度。
struct Cloud<'a> {
    /// 云端那份账（没有就是 `None`）—— 判"云端删没删"要它当证据
    account: Option<&'a Index>,
    /// 字节的封装/解封（端到端加密关着时就是原样进出）
    transform: &'a dyn SyncTransform,
    /// "整份重传"：忽略账本，全部按本地为准推上去
    force_upload: bool,
    /// 报进度（顺带续锁的那个包装也走这里，见调用处）
    progress: &'a dyn Fn(Progress),
}

/// 对账 + 动手（锁已经拿到了）
fn reconcile(
    s3: &S3,
    root: &Path,
    workspace: &Workspace,
    index: &mut Index,
    cloud: Cloud<'_>,
) -> Result<SyncReport, String> {
    let Cloud {
        account,
        transform,
        force_upload,
        progress,
    } = cloud;
    let mut report = SyncReport::default();

    // ---- 两边的清单 ----
    let local = scan_local(root)?;
    let remote: BTreeMap<String, Remote> = s3
        .list(&prefix_of(s3))
        .map_err(|error| format!("列不出云端清单：{error}"))?
        .into_iter()
        .filter_map(|object| {
            let relative = relative_of(s3, &object.key)?;
            Some((
                relative,
                Remote {
                    size: object.size,
                    etag: object.etag,
                    modified: parse_time(&object.modified)?,
                },
            ))
        })
        .collect();

    // ---- 一条条比 ----
    let mut paths: Vec<String> = local.keys().cloned().collect();
    paths.extend(remote.keys().cloned());
    paths.sort_unstable();
    paths.dedup();

    // 这一趟要从云端取回日志吗（云端有、本机没有的日志）—— 它决定内容块能不能删：
    // 那些日志可能正指着这些内容块，删了就"页面在、内容没了"。
    let restoring_logs = remote
        .keys()
        .any(|path| path.starts_with("db/objects/") && !local.contains_key(path));

    // 决定怎么办（纯规则）—— 判"删没删"要两处证据：云端那份账怎么说（`cloud`），
    // 以及本机这边有没有留下删除凭据（笔记的日志会进回收站）
    let planned: Vec<(String, Decision, String)> = paths
        .iter()
        .map(|path| {
            let evidence = Evidence {
                cloud: match account {
                    Some(cloud) if cloud.files.contains_key(path) => CloudSays::Listed,
                    Some(_) => CloudSays::Gone,
                    None => CloudSays::NoAccount,
                },
                // 内容块没有回收站：本机删它只有一条正当路径 —— 整理（GC）发现没人引用它。
                // 认不出"是整理删的"还是"工作目录被清过"，那就退一步问：
                // **本机缺着云端有的日志吗？** 缺，就说明这一趟要把日志取回来，
                // 而那些日志可能正指着这些内容块 —— 一个都不能删（取回来才对）。
                trashed: trashed_at(root, path)
                    || (path.starts_with("db/blobs/") && !restoring_logs),
            };
            let (decision, why) = decide(
                force_upload,
                local.get(path),
                remote.get(path),
                index.files.get(path),
                evidence,
            );
            (path.clone(), decision, why)
        })
        .collect();

    let work: Vec<&(String, Decision, String)> = planned
        .iter()
        .filter(|(_, decision, _)| !matches!(decision, Decision::Nothing))
        .collect();
    let total = work.len();

    // ---- 动手 ----
    for (done, (path, decision, why)) in work.iter().enumerate() {
        let text = describe(path, decision, why);
        progress(Progress {
            phase: match decision {
                Decision::Upload | Decision::DeleteRemote => "upload",
                _ => "download",
            }
            .to_string(),
            done,
            total,
            text,
        });

        // 隔一段把记账落一次盘（见 `INDEX_SAVE_EVERY`）
        if total > INDEX_SAVE_EVERY && done > 0 && done % INDEX_SAVE_EVERY == 0 {
            let _ = index.save(workspace);
        }

        match decision {
            Decision::Upload | Decision::TakeNewer("local") => {
                let bytes =
                    fs::read(root.join(path)).map_err(|error| format!("读不出 {path}：{error}"))?;
                let key = s3.config().key_of(path);
                let sealed = transform.seal(path, bytes)?;
                s3.put(&key, &sealed, false)
                    .map_err(|error| format!("传 {path} 失败：{error}"))?;
                // 传完记下云端的 ETag：下次比的就是它
                let etag = s3
                    .head(&key)
                    .map_err(|error| format!("看 {path} 失败：{error}"))?
                    .map(|object| object.etag)
                    .unwrap_or_default();
                report.uploaded += 1;
                report.bytes_up += sealed.len() as u64;
                stamp_local(index, root, path, etag);
                if matches!(decision, Decision::TakeNewer(_)) {
                    report.conflicts.push(Conflict {
                        path: path.clone(),
                        kept: "local".to_string(),
                    });
                }
            }

            Decision::Download | Decision::TakeNewer("remote") => {
                let key = s3.config().key_of(path);
                let Some(fetched) = s3
                    .get(&key)
                    .map_err(|error| format!("取 {path} 失败：{error}"))?
                else {
                    continue;
                };
                let plain = transform.open(path, fetched.bytes)?;
                write_local(root, path, &plain)?;
                report.downloaded += 1;
                report.bytes_down += plain.len() as u64;
                // 记的是**清单**里的那个 ETag，不是这次 GET 响应头里的：下一趟比的就是
                // 清单，两边得是同一路读来的。记成另一路的，那两路只要有一点出入
                // （引号写法、大小写、有没有这个头），每趟都会判"云端变了" —— 于是
                // 下载完再点同步，又是全量下载一遍。
                let etag = remote
                    .get(path)
                    .map(|held| held.etag.clone())
                    .unwrap_or_else(|| fetched.etag.clone());
                stamp_local(index, root, path, etag);
                if matches!(decision, Decision::TakeNewer(_)) {
                    report.conflicts.push(Conflict {
                        path: path.clone(),
                        kept: "remote".to_string(),
                    });
                }
            }

            Decision::DeleteRemote => {
                s3.delete(&s3.config().key_of(path))
                    .map_err(|error| format!("删云端 {path} 失败：{error}"))?;
                report.removed_remote += 1;
                index.files.remove(path);
            }

            Decision::DeleteLocal => {
                let _ = fs::remove_file(root.join(path));
                report.removed_local += 1;
                index.files.remove(path);
                let _ = why;
            }

            // `TakeNewer` 的另一半在下面那两支里处理了；这里不该再走到
            Decision::TakeNewer(_) | Decision::Nothing => {}
        }
    }

    progress(Progress {
        phase: "done".to_string(),
        done: total,
        total,
        text: "同步完成".to_string(),
    });

    Ok(report)
}

fn describe(path: &str, decision: &Decision, why: &str) -> String {
    let what = match decision {
        Decision::Upload | Decision::TakeNewer("local") => format!("上传 {path}"),
        Decision::Download | Decision::TakeNewer("remote") => format!("下载 {path}"),
        Decision::DeleteRemote => format!("云端删除 {path}"),
        Decision::DeleteLocal => format!("本机删除 {path}"),
        _ => path.to_string(),
    };
    if why.is_empty() {
        what
    } else {
        format!("{what}（{why}）")
    }
}

/// 记下这一份现在的样子（内容指纹、时间、云端的 ETag）
fn stamp_local(index: &mut Index, root: &Path, path: &str, etag: String) {
    let target = root.join(path);
    let Ok(meta) = fs::metadata(&target) else {
        return;
    };
    let hash = if path.starts_with("db/blobs/") {
        path.rsplit('/').next().unwrap_or_default().to_string()
    } else {
        content_hash(&target)
    };
    index.files.insert(
        path.to_string(),
        settings::Stamp {
            hash,
            mtime: mtime_of(&meta),
            etag,
        },
    );
}

/// 一份文件的内容指纹（内容块不必走这里，它们的名字就是指纹）
fn content_hash(path: &Path) -> String {
    use sha2::{Digest, Sha256};

    match fs::read(path) {
        Ok(bytes) => Sha256::digest(&bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
        Err(_) => String::new(),
    }
}

/// 写回本机（顺带把目录建出来 —— 云端有的那些目录，本机可能是空的）
fn write_local(root: &Path, path: &str, bytes: &[u8]) -> Result<(), String> {
    let target = root.join(path);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("建不出 {}：{error}", parent.display()))?;
    }
    crate::storage::workspace::write_bytes(&target, bytes)
        .map_err(|error| format!("写 {path} 失败：{error}"))
}

/// 本机这一份清单：只挑**该同步**的那些
fn scan_local(root: &Path) -> Result<BTreeMap<String, Local>, String> {
    let mut out = BTreeMap::new();
    walk(root, root, &mut out)?;
    Ok(out)
}

fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, Local>) -> Result<(), String> {
    let entries =
        fs::read_dir(dir).map_err(|error| format!("读不出 {}：{error}", dir.display()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            walk(root, &path, out)?;
            continue;
        }
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        let relative = relative.to_string_lossy().replace('\\', "/");
        if !is_synced(&relative) {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        let hash = if relative.starts_with("db/blobs/") {
            // 内容寻址：名字就是指纹，不必读内容
            relative.rsplit('/').next().unwrap_or_default().to_string()
        } else {
            content_hash(&path)
        };
        out.insert(
            relative,
            Local {
                hash,
                mtime: mtime_of(&meta),
            },
        );
    }
    Ok(())
}

/// 该同步的是哪些 —— 这个问题由**工作目录**那一层回答（见
/// [`crate::storage::workspace::is_synced`]）：工作目录长什么样是它的事，
/// 同步只管照办。往后加东西时也就不必来改这个模块。
fn is_synced(relative: &str) -> bool {
    crate::storage::workspace::is_synced(relative)
}

fn prefix_of(s3: &S3) -> String {
    let prefix = s3.config().prefix.trim_matches('/');
    if prefix.is_empty() {
        String::new()
    } else {
        format!("{prefix}/")
    }
}

/// 云端的键 → 工作目录里的相对路径；不该同步的那些返回 `None`
fn relative_of(s3: &S3, key: &str) -> Option<String> {
    let prefix = prefix_of(s3);
    let relative = key.strip_prefix(&prefix)?;
    // 锁与账本是云端自己的东西，不是仓库里的文件 —— 别把它们下载到工作目录里
    if relative.is_empty() || relative == LOCK_KEY || relative == LATEST_KEY || !is_synced(relative)
    {
        return None;
    }
    Some(relative.to_string())
}

/// 读云端那份账本（没有就是 `None`：第一次用、换了桶、或者还没写过）
fn fetch_latest(s3: &S3) -> Option<Index> {
    let key = s3.config().key_of(LATEST_KEY);
    let fetched = s3.get(&key).ok().flatten()?;
    serde_json::from_slice::<Index>(&fetched.bytes).ok()
}

/// 把跑完时的账本写到云端。
///
/// **明文**：里面只有路径、内容指纹与 ETag，没有一分内容；而路径本来就以键的形式
/// 摆在云端（键名就是相对路径），再封一层只是自欺欺人。
fn publish_latest(s3: &S3, index: &Index) -> Result<(), String> {
    let key = s3.config().key_of(LATEST_KEY);
    let body = serde_json::to_vec(index).map_err(|error| format!("账本序列化失败：{error}"))?;
    s3.put(&key, &body, false)?;
    Ok(())
}

/// 本机这边"这一份是被删掉的"凭据：笔记的日志进了回收站。
///
/// 只有 `db/objects/<命名空间>/<id>.log` 这种形状查得出来（回收站按命名空间存，
/// 见 `Database::trash_note_path`）。别的形状一律"不知道" —— 不知道就别替人删
/// （内容块那一类由 `reconcile` 另作判断，见那里的说明）。
fn trashed_at(root: &Path, path: &str) -> bool {
    let Some(rest) = path.strip_prefix("db/objects/") else {
        return false;
    };
    root.join("trash").join(rest).is_file()
}

fn mtime_of(meta: &fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|since| since.as_secs() as i64)
        .unwrap_or_default()
}

/// `2026-10-01T10:00:00.000Z` → Unix 秒；认不出就是 0（那一边就按"旧"处理）
fn parse_time(text: &str) -> Option<i64> {
    OffsetDateTime::parse(text, &time::format_description::well_known::Rfc3339)
        .ok()
        .map(|time| time.unix_timestamp())
}

impl Index {
    fn save(&self, workspace: &Workspace) -> Result<(), String> {
        write_json(&index_path(workspace), self)
    }
}
