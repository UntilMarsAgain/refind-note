//! 系统 gpg 那一层：签名、验签、加解密，以及"这台机器到底行不行"的探针。
//!
//! **手机上这一层不存在**：gpg 是桌面上的系统组件（还要 libgpgme 这个系统库），
//! Android / iOS 上装不了。所以下面那一整块在 mobile 上编译不进来，换成紧跟其后的
//! 几个同名的桩 —— 调用它们的地方照旧拿到一句"没有 gpg"，功能降级，编译照过。
//!
//! gpg 是**可选的**：机器上没有 gpg 时签名、加密、验签都用不了，其余（明文、压缩、
//! 口令对称）一切照常。这个模块同时是钥匙串那几件事的落点 —— 见 [`crate::features::keys`]。

#[cfg(any(desktop, test))]
use std::process::Command;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;

use super::{CodecError, Result};

/// 移动端：没有 gpg —— 签名、验签、加解密都用不了，其余功能一切照常
#[cfg(mobile)]
pub(super) fn gpg_sign(_content: &[u8], _key_id: &str) -> Result<String> {
    Err(CodecError::GpgUnavailable)
}

#[cfg(mobile)]
pub(super) fn gpg_verify(_content: &[u8], _signature: &[u8]) -> Result<VerifyOutcome> {
    Err(CodecError::GpgUnavailable)
}

#[cfg(mobile)]
pub(super) fn gpg_encrypt(_content: &[u8], _key_id: &str) -> Result<Vec<u8>> {
    Err(CodecError::GpgUnavailable)
}

#[cfg(mobile)]
pub(super) fn gpg_decrypt(_content: &[u8]) -> Result<Vec<u8>> {
    Err(CodecError::GpgUnavailable)
}

/// 这台计算机上有没有可用的 gpg。
///
/// 探测 PATH 里的 `gpg`（或 `gpg2`）命令。没有 gpg 时，签名 / 加密 / 验签这些功能
/// **不可用**：用到它们的地方给一句说得清的错，其余功能一切照常。
///
/// 每次调用都真去探一遍 —— gpg 的操作本来就要起进程，多这一次无妨；
/// 好处是用户中途装上 gpg，不用重启程序。
pub fn gpg_available() -> bool {
    // 手机上不可能有 gpg（那一层桌面才有，还要 libgpgme）：不必去探
    #[cfg(mobile)]
    {
        false
    }
    #[cfg(desktop)]
    {
        #[cfg(test)]
        if FORCE_NO_GPG.load(std::sync::atomic::Ordering::Relaxed) {
            return false;
        }

        ["gpg", "gpg2"].into_iter().any(|program| {
            Command::new(program)
                .arg("--version")
                .output()
                .map(|output| output.status.success())
                .unwrap_or(false)
        })
    }
}

/// 系统里的 gpg 是哪一个、什么版本（诊断页要报）；没有就是 `None`。
///
/// 只取版本那一行的原文 —— 排障时"是哪一家的 gpg、几版的"常常就是答案。
pub fn gpg_version() -> Option<String> {
    if !gpg_available() {
        return None;
    }

    #[cfg(desktop)]
    for program in ["gpg", "gpg2"] {
        if let Ok(output) = Command::new(program).arg("--version").output() {
            if output.status.success() {
                let text = String::from_utf8_lossy(&output.stdout);
                if let Some(first) = text.lines().next() {
                    let first = first.trim();
                    if !first.is_empty() {
                        return Some(first.to_string());
                    }
                }
            }
        }
    }

    None
}

/// 测试用：假装这台机器上没有 gpg（不假装就真去探）。
#[cfg(test)]
// 测试要能把它掰成"这台机器上没有 gpg"，所以静态本身得让同目录的 tests 够得着。
pub(super) static FORCE_NO_GPG: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// 测试用的 gpg 家目录。生产里不设它，就跟着系统配置走。
#[cfg(test)]
static TEST_GPG_HOME: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

thread_local! {
    /// 这一段里不许弹口令（渲染时会把它打开）
    static NON_INTERACTIVE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// 在这段里做加解密时**绝不弹口令**：gpg 拿不到就用错误回答，而不是叫 pinentry。
///
/// 谁需要它：**渲染**。渲染是"顺手看一眼"的动作（模板嵌入会去读别的页面），
/// 不该因为某一页没解锁就蹦出一个口令框、把界面卡在那儿等人输密码 ——
/// 那看起来就是"程序死了"。解锁是**用户按下去的**动作，只该由阅读那一页来触发。
pub fn without_prompting<T>(run: impl FnOnce() -> T) -> T {
    let previous = NON_INTERACTIVE.with(|flag| flag.replace(true));
    let out = run();
    NON_INTERACTIVE.with(|flag| flag.set(previous));
    out
}

/// 指定 gpg 的家目录（只有测试会调）
#[cfg(test)]
pub fn set_gpg_home(dir: std::path::PathBuf) {
    let _ = TEST_GPG_HOME.set(dir);
}

/// gpgme 的错说成人话。
///
/// 最要紧的是**取消**：口令输入框被关掉时 gpg 报的东西（`操作已取消 (gpg error 99)`）
/// 对用户毫无意义 —— 那不是故障，是"没输口令"。
#[cfg(desktop)]
fn gpg_error(action: &str, error: gpgme::Error) -> CodecError {
    // 取消是 gpg 自己的一种"错"（99）：拿它自己那个常量比，别写死数字
    if error.code() == gpgme::error::Error::CANCELED.code() {
        return CodecError::Gpg(format!("{action}已取消：口令没有输入"));
    }
    CodecError::Gpg(format!("{action}失败：{error}"))
}

#[cfg(desktop)]
pub(crate) fn gpg_context() -> Result<gpgme::Context> {
    if !gpg_available() {
        return Err(CodecError::GpgUnavailable);
    }

    let mut context = gpgme::Context::from_protocol(gpgme::Protocol::OpenPgp)
        .map_err(|error| CodecError::Gpg(error.to_string()))?;

    // 这一段里不许弹口令（见 `without_prompting`）：拿不到就直接失败，
    // 别去叫 pinentry —— 否则一个"顺手看一眼"的动作能把界面挂住等人输密码
    if NON_INTERACTIVE.with(|flag| flag.get()) {
        context
            .set_pinentry_mode(gpgme::PinentryMode::Error)
            .map_err(|error| CodecError::Gpg(error.to_string()))?;
    }

    #[cfg(test)]
    if let Some(home) = TEST_GPG_HOME.get() {
        context
            .set_engine_home_dir(home.to_string_lossy().to_string())
            .map_err(|error| CodecError::Gpg(error.to_string()))?;
    }
    // 载荷是二进制字节，不要 armored 文本（文件本身就是二进制容器）
    context.set_armor(false);
    context.set_text_mode(false);
    Ok(context)
}

#[cfg(desktop)]
pub(super) fn gpg_sign(content: &[u8], key_id: &str) -> Result<String> {
    let mut context = gpg_context()?;
    let key = context
        .get_secret_key(key_id)
        .map_err(|error| CodecError::Gpg(format!("找不到签名密钥 {key_id}：{error}")))?;
    context
        .add_signer(&key)
        .map_err(|error| CodecError::Gpg(format!("选定签名密钥失败：{error}")))?;

    let mut signature = Vec::new();
    context
        .sign(gpgme::SignMode::Detached, content, &mut signature)
        .map_err(|error| gpg_error("签名", error))?;

    Ok(BASE64.encode(signature))
}

/// 验签的结果。
///
/// 验签有两种"没通过"：签名确实对不上，和这步根本没跑起来（没有 gpg、读不动钥匙串）。
/// 前者是结论，后者是故障 —— 调用方要分得开，所以故障走 `Err`，结论走这里。
pub struct VerifyOutcome {
    /// 签名验过了没有
    pub verified: bool,
    /// 本地钥匙串对签名者公钥的信任程度（人话）
    pub trust: String,
    /// 人话说明：通过时报指纹，没通过时报原因
    pub detail: String,
}

#[cfg(desktop)]
pub(super) fn gpg_verify(content: &[u8], signature: &[u8]) -> Result<VerifyOutcome> {
    let mut context = gpg_context()?;
    // 注意参数顺序：第一个是**签名**，第二个才是被签的字节
    let result = context
        .verify_detached(signature, content)
        .map_err(|error| gpg_error("验签", error))?;

    let Some(found) = result.signatures().next() else {
        return Err(CodecError::Gpg("这份没有可验的签名".to_string()));
    };

    // 指纹取自签名本身，而不是头里记的那个名字 —— 名字是写的时候写上的，指纹是验出来的
    let fingerprint = found.fingerprint().ok().map(str::to_string);
    let trust = trust_label(found.validity());
    // 信任不够或公钥不在本地时，gpg 会另给一个原因；它比笼统的"验签失败"有用得多
    let reason = found.nonvalidity_reason().map(|error| error.to_string());

    Ok(match found.status() {
        Ok(()) => VerifyOutcome {
            verified: true,
            trust,
            detail: match (&fingerprint, &reason) {
                (Some(fingerprint), Some(reason)) => {
                    format!("签名通过（指纹 {fingerprint}；{reason}）")
                }
                (Some(fingerprint), None) => format!("签名通过（指纹 {fingerprint}）"),
                (None, _) => "签名通过".to_string(),
            },
        },
        Err(status) => VerifyOutcome {
            verified: false,
            trust,
            detail: match &reason {
                Some(reason) => format!("签名对不上：{status}（{reason}）"),
                None => format!("签名对不上：{status}"),
            },
        },
    })
}

/// 本地对签名者公钥的信任程度 → 人话
/// 一条用户标识 → `姓名 <邮箱>`：缺哪一半就给另一半，两样都没有就不算一条。
///
/// 放在这里是因为两个地方都要用：密钥列表（`features::keys`）与"这一版是谁签的"
/// （签名 / 加密报告）—— 同一条标识在两处必须长得一样。
#[cfg(desktop)]
pub fn uid_text(uid: gpgme::UserId<'_>) -> Option<String> {
    let name = uid.name().ok().unwrap_or_default();
    let email = uid.email().ok().unwrap_or_default();
    match (name.is_empty(), email.is_empty()) {
        (false, false) => Some(format!("{name} <{email}>")),
        (false, true) => Some(name.to_string()),
        (true, false) => Some(format!("<{email}>")),
        (true, true) => None,
    }
}

/// 钥匙串里这一把的**主**用户标识；查不到（没装 gpg、钥匙串里没有它）就是 `None`。
///
/// 只读公开信息，不解锁、不动私钥。
#[cfg(desktop)]
pub fn key_uid(key_id: &str) -> Option<String> {
    let mut context = gpg_context().ok()?;
    let key = context.get_key(key_id).ok()?;
    key.user_ids().find_map(uid_text)
}

/// 移动端：读不到钥匙串（没有 gpg）—— 与"钥匙串里没有它"是同一个答案
#[cfg(mobile)]
pub fn key_uid(_key_id: &str) -> Option<String> {
    None
}

#[cfg(desktop)]
pub(crate) fn trust_label(validity: gpgme::Validity) -> String {
    match validity {
        gpgme::Validity::Unknown => "本机没有这把公钥".to_string(),
        gpgme::Validity::Undefined => "这把公钥还没打信任分".to_string(),
        gpgme::Validity::Never => "这把公钥标着不受信任".to_string(),
        gpgme::Validity::Marginal => "勉强信任".to_string(),
        gpgme::Validity::Full => "完全信任".to_string(),
        gpgme::Validity::Ultimate => "绝对信任".to_string(),
    }
}

#[cfg(desktop)]
pub(super) fn gpg_encrypt(content: &[u8], key_id: &str) -> Result<Vec<u8>> {
    let mut context = gpg_context()?;
    let key = context
        .get_key(key_id)
        .map_err(|error| CodecError::Gpg(format!("找不到加密密钥 {key_id}：{error}")))?;

    let mut cipher = Vec::new();
    context
        .encrypt_with_flags(
            Some(&key),
            content,
            &mut cipher,
            gpgme::EncryptFlags::ALWAYS_TRUST,
        )
        .map_err(|error| gpg_error("加密", error))?;

    Ok(cipher)
}

#[cfg(desktop)]
pub(super) fn gpg_decrypt(content: &[u8]) -> Result<Vec<u8>> {
    let mut context = gpg_context()?;
    let mut plain = Vec::new();
    context
        .decrypt(content, &mut plain)
        .map_err(|error| gpg_error("解密", error))?;
    Ok(plain)
}
