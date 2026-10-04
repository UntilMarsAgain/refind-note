//! **云端那把锁**：动手之前先在云端抢一把，别的机器就等着。
//!
//! 手段是 S3 原生的原子操作 —— `PutObject` 带 `If-None-Match: *`（"不存在才写得进去"）。
//! 锁里写着机器名与时间（[`LockBody`]），超过 [`LOCK_TTL`] 没续的算过期，别人可以抢：机器
//! 崩了不该把同步永久锁死。
//!
//! 人也可以不等：界面上那颗「强制同步」把 `force` 传进来，锁还热着也照样抢（代价写在
//! 界面上：两台机器真在同步就会互相盖）。

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::storage::s3::S3;

use super::{LOCK_KEY, LOCK_TTL};

/// 拿在手里的锁
pub(super) struct Lock {
    key: String,
}

impl Lock {
    pub(super) fn release(self, s3: &S3) -> Result<(), String> {
        s3.delete(&self.key)
    }

    /// 续一下：把时间戳写成现在（长活儿跑到一半用，见 [`LOCK_REFRESH`](super::LOCK_REFRESH)）。
    ///
    /// 别的机器据此知道这边还活着 —— 一趟大同步不至于被当成"崩了"抢走。
    pub(super) fn touch(&self, s3: &S3) -> Result<(), String> {
        let body = lock_body()?;
        s3.put(&self.key, &body, false)?;
        Ok(())
    }
}

/// 锁里写的是什么（拿锁与续锁写的是同一份东西）
fn lock_body() -> Result<Vec<u8>, String> {
    let now = OffsetDateTime::now_utc().unix_timestamp();
    serde_json::to_vec(&LockBody {
        host: hostname(),
        at: now,
    })
    .map_err(|error| format!("锁序列化失败：{error}"))
}

/// 锁里写着什么（过期判断要读它）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct LockBody {
    /// 谁拿的（机器名，报告里给另一台看）
    pub(super) host: String,
    /// 什么时候拿的（Unix 秒）
    pub(super) at: i64,
}

/// 抢锁：`If-None-Match: *` 是 S3 原生的"不存在才写得进去"。
///
/// 已经有人拿着、而且没超过 [`LOCK_TTL`]：报错让这次同步别跑 —— 两台机器同时改
/// 会互相盖掉。过期了就抢过来（机器崩了不该把同步永久锁死）。
///
/// `force` 是**人**按下去的"不等了"（界面上那颗「强制同步」）：锁还热着也照样盖成
/// 自己的。另一台机器要是真在同步，两边就撞上了 —— 所以默认不走这条路，界面上也
/// 写着代价（宁可等它超时）。
pub(super) fn acquire(s3: &S3, force: bool) -> Result<Lock, String> {
    let key = s3.config().key_of(LOCK_KEY);
    let body = lock_body()?;

    if force {
        s3.put(&key, &body, false)?;
        return Ok(Lock { key });
    }

    let now = OffsetDateTime::now_utc().unix_timestamp();
    match s3.put(&key, &body, true)? {
        crate::storage::s3::PutOutcome::Written => Ok(Lock { key }),
        crate::storage::s3::PutOutcome::AlreadyExists | crate::storage::s3::PutOutcome::Changed => {
            // 有人拿着：看看过期没有
            let held = s3
                .get(&key)?
                .and_then(|fetched| serde_json::from_slice::<LockBody>(&fetched.bytes).ok());
            let stale = held
                .as_ref()
                .map(|body| now - body.at > LOCK_TTL)
                .unwrap_or(true);

            let who = held
                .as_ref()
                .map(|body| body.host.clone())
                .unwrap_or_else(|| "另一台机器".to_string());
            if !stale {
                return Err(format!(
                    "云端同步锁正被「{who}」拿着（它要是崩了，{LOCK_TTL} 秒后自动过期）；\
                     不想等就在设置页点「强制同步」抢过来"
                ));
            }

            // 过期的：删掉再抢一次。删与抢之间可能有人插进来 —— 那正好，他拿走便是
            s3.delete(&key)?;
            match s3.put(&key, &body, true)? {
                crate::storage::s3::PutOutcome::Written => Ok(Lock { key }),
                _ => Err("云端同步锁刚好被别的机器抢走了，过一会儿再试".to_string()),
            }
        }
    }
}

fn hostname() -> String {
    // 三个系统三套说法：Linux 认 `HOSTNAME`、Windows 认 `COMPUTERNAME`，
    // 再不行（比如 Android）读 `/etc/hostname`，都不行就报"某台机器" —— 锁上那个
    // 名字只是给另一台机器看的，没有它也一样排队。
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .or_else(|_| std::fs::read_to_string("/etc/hostname").map(|text| text.trim().to_string()))
        .ok()
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "某台机器".to_string())
}
