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
//! 封装格式：一层层包起来，**读的时候照头解**。
//!
//! ```text
//! 文件 = magic(4) | 格式版本(1) | 头长(4, BE) | 头(JSON, 明文) | 载荷
//!
//! 头 = { layers: [ …层链，内 → 外… ], meta: { mime } }
//!
//! 层序（内 → 外）：压缩 → GPG → 对称加密
//!
//! **算法写在头里**（压缩用哪种、口令层用哪种），所以同一份格式能容下好几档算法；
//! 读的时候照头上那一档解，与当前设置无关 —— 换算法只影响此后写的那些。
//! ```
//!
//! 四条规矩：
//!
//! - **头是明文**：所以"压没压、签没签、用哪把钥匙、加没加密、里面是什么"不输口令就能报出来。
//! - **读只照头**，永不照当前策略 —— 这就是"所有模式都要能读取"的落实方式。
//! - **未认识的层 id 明确报出来**，而不是当文件损坏 —— 前向兼容的出口。
//! - **meta 只是声明**（内容的 mime），认不得的字段忽略；它进头就进地址，
//!   所以同内容、不同 meta 会落成两个对象（与"签过名的不去重"同一条规则）。
//!
//! gpg 是**可选的**：机器上没有 gpg 时，签名、加密、验签这些功能不可用（用到时给一句
//! 说得清的错），其余（明文、压缩、口令对称）一切照常。
//!
//! 压缩为什么在最内层：加密之后信息熵已经拉满，再压只会让体积略微变大还多烧一遍 CPU。
//!
//! ## 这一层怎么分的
//!
//! | 文件 | 管什么 |
//! |---|---|
//! | 本文件 | 格式本身：头怎么读写、层链怎么按序套上去与拆下来、错误类型 |
//! | [`types`] | 算法与层链这些**类型**，以及那几份自述（一个字节都不碰） |
//! | [`compress`] | 压缩层：deflate / brotli |
//! | [`symmetric`] | 口令层：Argon2id 派生 + AEAD |
//! | [`gpg`] | 系统 gpg 那一层，含移动端的同名桩 |
//!
//! 对外的路径一律是 `codec::X` —— 下面的 `pub use` 就是为了这件事：类型住在子模块里，
//! 用起来却仍然只写 `codec::Cipher`，不必知道它究竟在哪个文件。

mod compress;
mod gpg;
mod symmetric;
mod types;

#[cfg(test)]
mod tests;

// 重导出是为了让对外路径停在 `codec::X` —— 类型住在子模块里，用起来不必知道它在哪个文件。
// 下面几个带 `cfg` 的重导出要**跟着被重导出的东西一起带条件**，否则桌面/移动端两边
// 各有一半路径对不上。
#[cfg(test)]
pub use gpg::set_gpg_home;
pub use gpg::{gpg_available, gpg_version, key_uid, without_prompting};
// `uid_text` 只有桌面版有：它要把 `gpgme::UserId` 这个**桌面才有的类型**
// 拆成"名字 <邮箱>"，而 `gpgme` 整个 crate 在移动端就不参与编译（见 Cargo.toml）。
// 移动端的调用点只有 `features::keys::list` 里那一处 `filter_map`，而那个函数
// 本身是 `#[cfg(desktop)]` 的 —— 所以移动端既用不到、也没有 `gpgme::UserId`
// 可以拿来造桩。给它写个空桩反而会让"这里到底该不该走"变得看不出来。
//
// （Android 构建时报的就是这个：`no uid_text in storage::codec::gpg`。）
#[cfg(desktop)]
pub use gpg::uid_text;
pub use gpg::VerifyOutcome;
pub use types::*;

// 这两个只在本仓库内用得到（`features/keys.rs` 与测试），所以重导出的可见性也只到
// crate —— 它们本来就是 `pub(crate)`，这里跟着，别顺手放大成 `pub`。
#[cfg(desktop)]
pub(crate) use gpg::{gpg_context, trust_label};

use std::fmt;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;

use compress::{compress_with, decompress_with};
use gpg::{gpg_decrypt, gpg_encrypt, gpg_sign, gpg_verify};
use symmetric::{symmetric_open, symmetric_seal};

/// 认得这个文件是本程序的封装格式
pub const MAGIC: &[u8; 4] = b"RNDB";
/// 格式版本。**改头的结构就要动它**
pub const FORMAT_VERSION: u8 = 1;

/// 口令不对（或密文被改过）时给的那句话。
///
/// 上层要凭它认出"这值得让人重输一次口令"，所以**只能有一处**写这句话。
pub const WRONG_PASSPHRASE_MESSAGE: &str = "口令不对，或者文件被改过";

const PREFIX_BYTES: usize = 9;
const SALT_BYTES: usize = 16;
const NONCE_BYTES: usize = 12;
const KEY_BYTES: usize = 32;

/// 口令派生的默认代价（OWASP 对 Argon2id 的推荐量级）
const DEFAULT_M_COST: u32 = 19_456;
const DEFAULT_T_COST: u32 = 2;
const DEFAULT_P_COST: u32 = 1;

// ---------------------------------------------------------------- 错误

#[derive(Debug, PartialEq, Eq)]
pub enum CodecError {
    /// 不是本程序的封装格式（或版本不认得）
    NotEnvelope(String),
    /// 头里出现了本程序不认识的层
    UnknownLayer(String),
    /// 这一层需要口令，但没人给
    PassphraseNeeded,
    /// 口令不对（或数据被改过）
    WrongPassphrase,
    /// 系统 gpg 那边出的问题
    Gpg(String),
    /// 这台计算机上没有 gpg（签名 / 加密 / 验签都用不了）
    GpgUnavailable,
    /// 载荷本身不成形
    Corrupt(String),
}

impl fmt::Display for CodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotEnvelope(reason) => write!(f, "不是本程序的封装格式：{reason}"),
            Self::UnknownLayer(id) => write!(f, "这个文件用了本程序不认识的层「{id}」"),
            Self::PassphraseNeeded => write!(f, "这一份是加密的，需要输入口令"),
            Self::WrongPassphrase => write!(f, "{WRONG_PASSPHRASE_MESSAGE}"),
            Self::Gpg(reason) => write!(f, "gpg 那边出错了：{reason}"),
            Self::GpgUnavailable => {
                write!(f, "这台计算机上没有 gpg：签名、加密与验签都用不了")
            }
            Self::Corrupt(reason) => write!(f, "文件不成形：{reason}"),
        }
    }
}

impl std::error::Error for CodecError {}

type Result<T> = std::result::Result<T, CodecError>;

// ---------------------------------------------------------------- 编码

/// 按策略把内容层层包起来。返回可以直接落盘的字节。
pub fn encode(
    content: &[u8],
    meta: &Meta,
    policy: &Policy,
    passphrase: Option<&str>,
) -> Result<Vec<u8>> {
    let mut payload = content.to_vec();
    let mut layers: Vec<Layer> = Vec::new();

    // 最内：压缩
    if policy.compress {
        let algorithm = policy.compression;
        let level = algorithm.default_level();
        payload = compress_with(algorithm, &payload, level)?;
        layers.push(Layer::Compress { algorithm, level });
    }

    // 中间：gpg（先签名，再加密 —— 签名盖的是压缩后的内容）
    if let Some(key) = &policy.gpg_sign {
        let signature = gpg_sign(&payload, key)?;
        layers.push(Layer::Gpg {
            mode: GpgMode::Sign,
            key: key.clone(),
            signature: Some(signature),
        });
    }
    if let Some(key) = &policy.gpg_encrypt {
        payload = gpg_encrypt(&payload, key)?;
        layers.push(Layer::Gpg {
            mode: GpgMode::Encrypt,
            key: key.clone(),
            signature: None,
        });
    }

    // 最外：口令对称
    if policy.symmetric {
        let passphrase = passphrase.ok_or(CodecError::PassphraseNeeded)?;
        let (sealed, layer) = symmetric_seal(&payload, passphrase, policy.cipher)?;
        payload = sealed;
        layers.push(layer);
    }

    assemble(&layers, meta, payload)
}

fn assemble(layers: &[Layer], meta: &Meta, payload: Vec<u8>) -> Result<Vec<u8>> {
    let header = Header {
        layers: layers.to_vec(),
        meta: meta.clone(),
    };
    let header = serde_json::to_vec(&header)
        .map_err(|error| CodecError::Corrupt(format!("头序列化失败：{error}")))?;

    let mut out = Vec::with_capacity(PREFIX_BYTES + header.len() + payload.len());
    out.extend_from_slice(MAGIC);
    out.push(FORMAT_VERSION);
    out.extend_from_slice(&(header.len() as u32).to_be_bytes());
    out.extend_from_slice(&header);
    out.extend_from_slice(&payload);
    Ok(out)
}

// ---------------------------------------------------------------- 解码

/// 只看头：保护状态与内容自述。**不需要口令**。
pub fn inspect(file: &[u8]) -> Result<Inspection> {
    let header = read_header(file)?;
    let mut protection = Protection {
        compress: false,
        compression: Compression::default(),
        sign: None,
        encrypt: None,
        symmetric: false,
        cipher: Cipher::default(),
    };

    for layer in &header.layers {
        match layer {
            Layer::Compress { algorithm, .. } => {
                protection.compress = true;
                protection.compression = *algorithm;
            }
            Layer::Gpg {
                mode: GpgMode::Sign,
                key,
                ..
            } => protection.sign = Some(key.clone()),
            Layer::Gpg {
                mode: GpgMode::Encrypt,
                key,
                ..
            } => protection.encrypt = Some(key.clone()),
            Layer::Symmetric { cipher, .. } => {
                protection.symmetric = true;
                protection.cipher = *cipher;
            }
        }
    }

    Ok(Inspection {
        protection,
        meta: header.meta,
    })
}

/// 验一遍 blob 里的签名层，不改变内容。没有签名层时返回 `None`。
///
/// 与 `decode` 一样要逐层往里走 —— 签名盖的是**它里面那一层**的输出，
/// 所以外层要是有口令层，同样得给出才能走得到签名层。
pub fn signature_report(file: &[u8], secrets: &Secrets<'_>) -> Result<Option<SignatureReport>> {
    let header = read_header(file)?;
    let mut payload = payload_of(file)?.to_vec();

    for layer in header.layers.iter().rev() {
        match layer {
            Layer::Symmetric {
                cipher,
                salt,
                nonce,
                m_cost,
                t_cost,
                p_cost,
            } => {
                let passphrase = secrets.passphrase.ok_or(CodecError::PassphraseNeeded)?;
                payload = symmetric_open(
                    &payload, passphrase, *cipher, salt, nonce, *m_cost, *t_cost, *p_cost,
                )?;
            }
            Layer::Gpg {
                mode: GpgMode::Encrypt,
                ..
            } => payload = gpg_decrypt(&payload)?,
            Layer::Gpg {
                mode: GpgMode::Sign,
                key,
                signature: Some(signature),
            } => {
                let raw = BASE64
                    .decode(signature)
                    .map_err(|error| CodecError::Corrupt(format!("签名不是 base64：{error}")))?;
                return Ok(Some(match gpg_verify(&payload, &raw) {
                    Ok(outcome) => SignatureReport {
                        key: key.clone(),
                        uid: key_uid(key),
                        verified: outcome.verified,
                        trust: Some(outcome.trust),
                        detail: outcome.detail,
                    },
                    // 验签这步自己就没跑起来（没有 gpg、钥匙串读不动）：如实说，不当成"签名不对"
                    Err(error) => SignatureReport {
                        key: key.clone(),
                        uid: key_uid(key),
                        verified: false,
                        trust: None,
                        detail: error.to_string(),
                    },
                }));
            }
            Layer::Gpg {
                mode: GpgMode::Sign,
                signature: None,
                ..
            } => return Err(CodecError::Corrupt("签名层没带签名".to_string())),
            Layer::Compress { algorithm, .. } => payload = decompress_with(*algorithm, &payload)?,
        }
    }

    Ok(None)
}

/// 本机认不认得这把钥匙。只看钥匙串里有没有，不去解任何东西。
#[cfg(desktop)]
pub fn encryption_report(key: &str) -> Result<EncryptionReport> {
    let mut context = gpg_context()?;

    // 有私钥才解得开；只有公钥说明这一份是"加密给别人"的，本机读不了
    let secret = context.get_secret_key(key).is_ok();
    let detail = if secret {
        "本机有这把私钥：输入它的口令就能解开".to_string()
    } else if context.get_key(key).is_ok() {
        "本机只有公钥、没有私钥：这一份本机解不开".to_string()
    } else {
        "本机没有这把钥匙：这一份本机解不开".to_string()
    };

    Ok(EncryptionReport {
        key: key.to_string(),
        uid: key_uid(key),
        secret,
        detail,
    })
}

/// 移动端：没有 gpg —— 这一份解不开，如实说（文案与桌面端"本机没有这把钥匙"一致）
#[cfg(mobile)]
pub fn encryption_report(key: &str) -> Result<EncryptionReport> {
    Ok(EncryptionReport {
        key: key.to_string(),
        uid: None,
        secret: false,
        detail: "这台设备上没有 gpg：这一份解不开（GPG 是桌面上的功能）".to_string(),
    })
}

/// 照头逐层解开。顺序是**从外往内**。
pub fn decode(file: &[u8], secrets: &Secrets<'_>) -> Result<Vec<u8>> {
    let header = read_header(file)?;
    let mut payload = payload_of(file)?.to_vec();

    for layer in header.layers.iter().rev() {
        match layer {
            Layer::Symmetric {
                cipher,
                salt,
                nonce,
                m_cost,
                t_cost,
                p_cost,
            } => {
                let passphrase = secrets.passphrase.ok_or(CodecError::PassphraseNeeded)?;
                payload = symmetric_open(
                    &payload, passphrase, *cipher, salt, nonce, *m_cost, *t_cost, *p_cost,
                )?;
            }
            Layer::Gpg {
                mode: GpgMode::Encrypt,
                ..
            } => payload = gpg_decrypt(&payload)?,
            Layer::Gpg {
                mode: GpgMode::Sign,
                signature: Some(signature),
                ..
            } => {
                // 签名不改载荷，只验
                let raw = BASE64
                    .decode(signature)
                    .map_err(|error| CodecError::Corrupt(format!("签名不是 base64：{error}")))?;
                let outcome = gpg_verify(&payload, &raw)?;
                if !outcome.verified {
                    return Err(CodecError::Gpg(outcome.detail));
                }
            }
            Layer::Gpg {
                mode: GpgMode::Sign,
                signature: None,
                ..
            } => return Err(CodecError::Corrupt("签名层没带签名".to_string())),
            Layer::Compress { algorithm, .. } => payload = decompress_with(*algorithm, &payload)?,
        }
    }

    Ok(payload)
}

/// 读明文头：层链（内 → 外）与内容自述。
pub fn read_header(file: &[u8]) -> Result<Header> {
    if file.len() < PREFIX_BYTES {
        return Err(CodecError::NotEnvelope("文件太短".to_string()));
    }
    if &file[..4] != MAGIC {
        return Err(CodecError::NotEnvelope("开头不是 RNDB".to_string()));
    }
    if file[4] != FORMAT_VERSION {
        return Err(CodecError::NotEnvelope(format!(
            "格式版本 {} 这个程序不认得（只认得 {FORMAT_VERSION}）",
            file[4]
        )));
    }

    let len = u32::from_be_bytes([file[5], file[6], file[7], file[8]]) as usize;
    let end = PREFIX_BYTES + len;
    if file.len() < end {
        return Err(CodecError::Corrupt("头被截断了".to_string()));
    }

    let raw: serde_json::Value = serde_json::from_slice(&file[PREFIX_BYTES..end])
        .map_err(|error| CodecError::Corrupt(format!("头读不出来：{error}")))?;

    // 先按名字认一遍：认不出的层要**点名**说是哪个，而不是含糊地报"文件坏了"。
    // 这是前向兼容的出口 —— 新版本写了新层，老版本得说得清自己缺什么。
    if let Some(items) = raw.get("layers").and_then(|value| value.as_array()) {
        for item in items {
            if let Some(name) = item.get("layer").and_then(|value| value.as_str()) {
                if !KNOWN_LAYERS.contains(&name) {
                    return Err(CodecError::UnknownLayer(name.to_string()));
                }
            }
        }
    }

    serde_json::from_value(raw).map_err(|error| CodecError::Corrupt(format!("头读不出来：{error}")))
}

/// 本程序认得的层名。加新层时这里要一起加 —— 认不出的名字要**点名**报出来，
/// 那是前向兼容的出口（比含糊地报"文件坏了"有用）。
const KNOWN_LAYERS: [&str; 3] = ["compress", "gpg", "symmetric"];

fn payload_of(file: &[u8]) -> Result<&[u8]> {
    if file.len() < PREFIX_BYTES {
        return Err(CodecError::NotEnvelope("文件太短".to_string()));
    }
    let len = u32::from_be_bytes([file[5], file[6], file[7], file[8]]) as usize;
    let start = PREFIX_BYTES + len;
    if file.len() < start {
        return Err(CodecError::Corrupt("头被截断了".to_string()));
    }
    Ok(&file[start..])
}
