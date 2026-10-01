//! 封装格式：一层层包起来，**读的时候照头解**。
//!
//! ```text
//! 文件 = magic(4) | 格式版本(1) | 头长(4, BE) | 头(JSON, 明文) | 载荷
//!
//! 头 = { layers: [ …层链，内 → 外… ], meta: { mime } }
//!
//! 层序（内 → 外）：压缩 → GPG → 对称加密
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

use std::fmt;
use std::io::Write;
use std::process::Command;

use argon2::{Algorithm, Argon2, Params, Version};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Nonce};
use flate2::write::{DeflateDecoder, DeflateEncoder};
use flate2::Compression;
use serde::{Deserialize, Serialize};

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

/// 压缩的默认档位
const DEFAULT_LEVEL: u32 = 6;
/// 口令派生的默认代价（OWASP 对 Argon2id 的推荐量级）
const DEFAULT_M_COST: u32 = 19_456;
const DEFAULT_T_COST: u32 = 2;
const DEFAULT_P_COST: u32 = 1;

// ---------------------------------------------------------------- 层

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "layer", rename_all = "kebab-case")]
pub enum Layer {
    /// 压缩
    Deflate { level: u32 },
    /// 系统 gpg。
    ///
    /// 签名是**分离签名**：它不改载荷，只把签名放在头里 —— 于是未加密的 blob
    /// 不需要任何口令就能验签。加密则改载荷。
    Gpg {
        mode: GpgMode,
        /// 密钥指纹（或 gpg 认得的任何标识）
        key: String,
        /// 只有签名层有
        #[serde(default, skip_serializing_if = "Option::is_none")]
        signature: Option<String>,
    },
    /// 口令对称加密。口令由用户输入，参数存这里 —— 少了任何一个都解不开。
    Symmetric {
        salt: String,
        nonce: String,
        m_cost: u32,
        t_cost: u32,
        p_cost: u32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GpgMode {
    Sign,
    Encrypt,
}

/// blob 的自述：**内容本身是什么**。
///
/// 角色（笔记 / 附件 / 代码）不在这里 —— 那是引用方（日志、附件表）的事。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Meta {
    /// 内容的媒体类型，如 `text/markdown`、`image/png`
    pub mime: String,
}

impl Default for Meta {
    fn default() -> Self {
        Self {
            mime: "application/octet-stream".to_string(),
        }
    }
}

/// 头：层链 + 内容自述，两者都是明文。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Header {
    /// 层链（内 → 外）
    pub layers: Vec<Layer>,
    /// 内容自述
    #[serde(default)]
    pub meta: Meta,
}

/// 一个 blob 的两样自述（`inspect` 的产物）
#[derive(Debug, Clone, PartialEq)]
pub struct Inspection {
    pub protection: Protection,
    pub meta: Meta,
}

/// 一份签名层的校验报告（`signature_report` 的产物）
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SignatureReport {
    /// 签名者密钥标识（**写这一版时指定的**那一把，记在头里）
    pub key: String,
    /// 签名验过了没有
    pub verified: bool,
    /// 本地钥匙串对签名者公钥的信任程度（人话）。
    /// 验签这步没能跑起来时说不出来，所以是可选的
    pub trust: Option<String>,
    /// 人话说明：通过时说指纹；不通过说原因
    pub detail: String,
}

/// 加密层的现状：加密到谁，以及本机对付不对付得了。
///
/// 只看**本地钥匙串**，不动数据 —— 所以它不解锁也说得出来。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EncryptionReport {
    /// 写这一版时指定的加密密钥（头里记的那个）
    pub key: String,
    /// 本机有没有对应的**私钥** —— 有才解得开
    pub secret: bool,
    /// 人话说明
    pub detail: String,
}

/// 一个 blob 声明用了哪些层 —— **明文头里就有，不需要口令**
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Protection {
    pub compress: bool,
    /// 签名者密钥指纹
    pub sign: Option<String>,
    /// 加密到的密钥指纹
    pub encrypt: Option<String>,
    /// 是否套了口令对称层
    pub symmetric: bool,
}

impl Protection {
    /// 什么都没做：落盘的就是明文
    #[cfg(test)]
    pub fn is_plain(&self) -> bool {
        !self.compress && self.sign.is_none() && self.encrypt.is_none() && !self.symmetric
    }
}

/// 写的时候照它来。**读的时候不看它**。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Policy {
    pub compress: bool,
    /// 用哪把密钥签名；`None` = 不签
    pub gpg_sign: Option<String>,
    /// 用哪把密钥加密；`None` = 不加密
    pub gpg_encrypt: Option<String>,
    /// 要不要再套一层口令对称加密
    pub symmetric: bool,
}

/// 解开某些层需要的秘密
#[derive(Default)]
pub struct Secrets<'a> {
    /// 对称层的口令
    pub passphrase: Option<&'a str>,
}

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
        payload = deflate(&payload, DEFAULT_LEVEL)?;
        layers.push(Layer::Deflate {
            level: DEFAULT_LEVEL,
        });
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
        let (sealed, layer) = symmetric_seal(&payload, passphrase)?;
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
        sign: None,
        encrypt: None,
        symmetric: false,
    };

    for layer in &header.layers {
        match layer {
            Layer::Deflate { .. } => protection.compress = true,
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
            Layer::Symmetric { .. } => protection.symmetric = true,
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
                salt,
                nonce,
                m_cost,
                t_cost,
                p_cost,
            } => {
                let passphrase = secrets.passphrase.ok_or(CodecError::PassphraseNeeded)?;
                payload =
                    symmetric_open(&payload, passphrase, salt, nonce, *m_cost, *t_cost, *p_cost)?;
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
                        verified: outcome.verified,
                        trust: Some(outcome.trust),
                        detail: outcome.detail,
                    },
                    // 验签这步自己就没跑起来（没有 gpg、钥匙串读不动）：如实说，不当成"签名不对"
                    Err(error) => SignatureReport {
                        key: key.clone(),
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
            Layer::Deflate { .. } => payload = inflate(&payload)?,
        }
    }

    Ok(None)
}

/// 本机认不认得这把钥匙。只看钥匙串里有没有，不去解任何东西。
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
        secret,
        detail,
    })
}

/// 照头逐层解开。顺序是**从外往内**。
pub fn decode(file: &[u8], secrets: &Secrets<'_>) -> Result<Vec<u8>> {
    let header = read_header(file)?;
    let mut payload = payload_of(file)?.to_vec();

    for layer in header.layers.iter().rev() {
        match layer {
            Layer::Symmetric {
                salt,
                nonce,
                m_cost,
                t_cost,
                p_cost,
            } => {
                let passphrase = secrets.passphrase.ok_or(CodecError::PassphraseNeeded)?;
                payload =
                    symmetric_open(&payload, passphrase, salt, nonce, *m_cost, *t_cost, *p_cost)?;
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
            Layer::Deflate { .. } => payload = inflate(&payload)?,
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

/// 本程序认得的三层。加新层时这里要一起加。
const KNOWN_LAYERS: [&str; 3] = ["deflate", "gpg", "symmetric"];

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

// ---------------------------------------------------------------- 各层的实现

fn deflate(content: &[u8], level: u32) -> Result<Vec<u8>> {
    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::new(level));
    encoder
        .write_all(content)
        .map_err(|error| CodecError::Corrupt(format!("压缩失败：{error}")))?;
    encoder
        .finish()
        .map_err(|error| CodecError::Corrupt(format!("压缩失败：{error}")))
}

fn inflate(payload: &[u8]) -> Result<Vec<u8>> {
    let mut decoder = DeflateDecoder::new(Vec::new());
    decoder
        .write_all(payload)
        .map_err(|error| CodecError::Corrupt(format!("解压失败：{error}")))?;
    decoder
        .finish()
        .map_err(|error| CodecError::Corrupt(format!("解压失败：{error}")))
}

fn symmetric_seal(content: &[u8], passphrase: &str) -> Result<(Vec<u8>, Layer)> {
    let mut salt = [0u8; SALT_BYTES];
    let mut nonce_bytes = [0u8; NONCE_BYTES];
    random_into(&mut salt)?;
    random_into(&mut nonce_bytes)?;

    let key = derive_key(
        passphrase,
        &salt,
        DEFAULT_M_COST,
        DEFAULT_T_COST,
        DEFAULT_P_COST,
    )?;
    let cipher = ChaCha20Poly1305::new_from_slice(&key)
        .map_err(|error| CodecError::Corrupt(format!("密钥长度不对：{error}")))?;
    let sealed = cipher
        .encrypt(Nonce::from_slice(&nonce_bytes), content)
        .map_err(|_| CodecError::WrongPassphrase)?;

    Ok((
        sealed,
        Layer::Symmetric {
            salt: BASE64.encode(salt),
            nonce: BASE64.encode(nonce_bytes),
            m_cost: DEFAULT_M_COST,
            t_cost: DEFAULT_T_COST,
            p_cost: DEFAULT_P_COST,
        },
    ))
}

#[allow(clippy::too_many_arguments)]
fn symmetric_open(
    payload: &[u8],
    passphrase: &str,
    salt: &str,
    nonce: &str,
    m_cost: u32,
    t_cost: u32,
    p_cost: u32,
) -> Result<Vec<u8>> {
    let salt = BASE64
        .decode(salt)
        .map_err(|error| CodecError::Corrupt(format!("salt 不是 base64：{error}")))?;
    let nonce = BASE64
        .decode(nonce)
        .map_err(|error| CodecError::Corrupt(format!("nonce 不是 base64：{error}")))?;
    if nonce.len() != NONCE_BYTES {
        return Err(CodecError::Corrupt("nonce 长度不对".to_string()));
    }

    let key = derive_key(passphrase, &salt, m_cost, t_cost, p_cost)?;
    let cipher = ChaCha20Poly1305::new_from_slice(&key)
        .map_err(|error| CodecError::Corrupt(format!("密钥长度不对：{error}")))?;
    cipher
        .decrypt(Nonce::from_slice(&nonce), payload)
        // 口令不对与密文被改过在这里是同一件事：AEAD 分不出来，也不该分
        .map_err(|_| CodecError::WrongPassphrase)
}

fn derive_key(
    passphrase: &str,
    salt: &[u8],
    m_cost: u32,
    t_cost: u32,
    p_cost: u32,
) -> Result<[u8; KEY_BYTES]> {
    let params = Params::new(m_cost, t_cost, p_cost, Some(KEY_BYTES))
        .map_err(|error| CodecError::Corrupt(format!("口令派生参数不对：{error}")))?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);

    let mut key = [0u8; KEY_BYTES];
    argon
        .hash_password_into(passphrase.as_bytes(), salt, &mut key)
        .map_err(|error| CodecError::Corrupt(format!("口令派生失败：{error}")))?;
    Ok(key)
}

fn random_into(buffer: &mut [u8]) -> Result<()> {
    getrandom::getrandom(buffer)
        .map_err(|error| CodecError::Corrupt(format!("取随机数失败：{error}")))
}

// ---------------------------------------------------------------- 系统 gpg

/// 这台计算机上有没有可用的 gpg。
///
/// 探测 PATH 里的 `gpg`（或 `gpg2`）命令。没有 gpg 时，签名 / 加密 / 验签这些功能
/// **不可用**：用到它们的地方给一句说得清的错，其余功能一切照常。
///
/// 每次调用都真去探一遍 —— gpg 的操作本来就要起进程，多这一次无妨；
/// 好处是用户中途装上 gpg，不用重启程序。
pub fn gpg_available() -> bool {
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

/// 测试用：假装这台机器上没有 gpg（不假装就真去探）。
#[cfg(test)]
static FORCE_NO_GPG: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 测试用的 gpg 家目录。生产里不设它，就跟着系统配置走。
#[cfg(test)]
static TEST_GPG_HOME: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

/// 指定 gpg 的家目录（只有测试会调）
#[cfg(test)]
pub fn set_gpg_home(dir: std::path::PathBuf) {
    let _ = TEST_GPG_HOME.set(dir);
}

/// gpgme 的错说成人话。
///
/// 最要紧的是**取消**：口令输入框被关掉时 gpg 报的东西（`操作已取消 (gpg error 99)`）
/// 对用户毫无意义 —— 那不是故障，是"没输口令"。
fn gpg_error(action: &str, error: gpgme::Error) -> CodecError {
    // 取消是 gpg 自己的一种"错"（99）：拿它自己那个常量比，别写死数字
    if error.code() == gpgme::error::Error::CANCELED.code() {
        return CodecError::Gpg(format!("{action}已取消：口令没有输入"));
    }
    CodecError::Gpg(format!("{action}失败：{error}"))
}

pub(crate) fn gpg_context() -> Result<gpgme::Context> {
    if !gpg_available() {
        return Err(CodecError::GpgUnavailable);
    }

    let mut context = gpgme::Context::from_protocol(gpgme::Protocol::OpenPgp)
        .map_err(|error| CodecError::Gpg(error.to_string()))?;

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

fn gpg_sign(content: &[u8], key_id: &str) -> Result<String> {
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

fn gpg_verify(content: &[u8], signature: &[u8]) -> Result<VerifyOutcome> {
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

fn gpg_encrypt(content: &[u8], key_id: &str) -> Result<Vec<u8>> {
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

fn gpg_decrypt(content: &[u8]) -> Result<Vec<u8>> {
    let mut context = gpg_context()?;
    let mut plain = Vec::new();
    context
        .decrypt(content, &mut plain)
        .map_err(|error| gpg_error("解密", error))?;
    Ok(plain)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// gpg 的可用性是进程级开关，碰它的用例要串行。
    fn gpg_guard() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[test]
    fn no_layers_is_wrapped_but_readable() {
        // 什么都不做也要有头："裸字节"只是"空链"，不存在"有时有头有时没头"
        let file = encode(b"hello", &Meta::default(), &Policy::default(), None).unwrap();

        assert_eq!(&file[..4], MAGIC);
        assert!(inspect(&file).unwrap().protection.is_plain());
        assert_eq!(decode(&file, &Secrets::default()).unwrap(), b"hello");
    }

    #[test]
    fn compression_round_trips() {
        let content = "重复的内容。".repeat(200);
        let policy = Policy {
            compress: true,
            ..Default::default()
        };

        let file = encode(content.as_bytes(), &Meta::default(), &policy, None).unwrap();
        assert!(file.len() < content.len(), "压缩该让体积变小");
        assert!(inspect(&file).unwrap().protection.compress);
        assert_eq!(
            decode(&file, &Secrets::default()).unwrap(),
            content.as_bytes()
        );
    }

    #[test]
    fn symmetric_round_trips_and_needs_the_right_passphrase() {
        let content = b"only for us".to_vec();
        let policy = Policy {
            symmetric: true,
            ..Default::default()
        };

        let file = encode(&content, &Meta::default(), &policy, Some("open sesame")).unwrap();
        // 内容是看不见的
        assert!(!file.windows(11).any(|window| window == b"only for us"));

        // 头是明文：不输口令也知道这是加密的
        assert!(inspect(&file).unwrap().protection.symmetric);

        // 没给口令
        assert!(matches!(
            decode(&file, &Secrets::default()),
            Err(CodecError::PassphraseNeeded)
        ));
        // 口令不对
        assert!(matches!(
            decode(
                &file,
                &Secrets {
                    passphrase: Some("wrong")
                }
            ),
            Err(CodecError::WrongPassphrase)
        ));
        // 对的口令
        assert_eq!(
            decode(
                &file,
                &Secrets {
                    passphrase: Some("open sesame")
                }
            )
            .unwrap(),
            content
        );
    }

    #[test]
    fn the_whole_chain_nests_in_the_right_order() {
        // 压缩 + 对称：先压后加，读的时候反过来
        let content = "一二三四五六七八九十".repeat(50);
        let policy = Policy {
            compress: true,
            symmetric: true,
            ..Default::default()
        };

        let file = encode(content.as_bytes(), &Meta::default(), &policy, Some("pw")).unwrap();
        let protection = inspect(&file).unwrap().protection;
        assert!(protection.compress && protection.symmetric);

        // 层链是从内到外的：压缩在内，对称在外
        let header = read_header(&file).unwrap();
        assert!(matches!(header.layers[0], Layer::Deflate { .. }));
        assert!(matches!(header.layers[1], Layer::Symmetric { .. }));

        assert_eq!(
            decode(
                &file,
                &Secrets {
                    passphrase: Some("pw")
                }
            )
            .unwrap(),
            content.as_bytes()
        );
    }

    #[test]
    fn a_foreign_file_is_told_apart_from_a_corrupt_one() {
        assert!(matches!(
            decode(b"not ours", &Secrets::default()),
            Err(CodecError::NotEnvelope(_))
        ));

        // 版本不认得也算"不是我们的"
        let mut file = encode(b"x", &Meta::default(), &Policy::default(), None).unwrap();
        file[4] = 99;
        assert!(matches!(inspect(&file), Err(CodecError::NotEnvelope(_))));

        // 头被截断才算损坏
        let file = encode(b"x", &Meta::default(), &Policy::default(), None).unwrap();
        assert!(matches!(
            inspect(&file[..6]),
            Err(CodecError::NotEnvelope(_))
        ));
    }

    #[test]
    fn an_unknown_layer_is_reported_by_name() {
        // 加了新层时，老程序要说得清"是我不认识"，而不是"文件坏了"
        let header = br#"{"layers":[{"layer":"quantum","qbits":7}]}"#;
        let mut file = Vec::new();
        file.extend_from_slice(MAGIC);
        file.push(FORMAT_VERSION);
        file.extend_from_slice(&(header.len() as u32).to_be_bytes());
        file.extend_from_slice(header);
        file.extend_from_slice(b"payload");

        let error = inspect(&file).unwrap_err();
        assert_eq!(error, CodecError::UnknownLayer("quantum".to_string()));
        assert!(error.to_string().contains("quantum"), "{error}");
    }

    #[test]
    fn the_header_carries_the_content_mime() {
        let meta = Meta {
            mime: "image/png".to_string(),
        };
        let file = encode(b"\x89PNG", &meta, &Policy::default(), None).unwrap();

        // 头是明文：不解层就知道里面是什么
        assert_eq!(read_header(&file).unwrap().meta.mime, "image/png");
        assert_eq!(inspect(&file).unwrap().meta.mime, "image/png");
        // 内容照常解出来
        assert_eq!(decode(&file, &Secrets::default()).unwrap(), b"\x89PNG");
    }

    #[test]
    fn an_unknown_meta_key_is_ignored() {
        // meta 只是声明：认不得的字段忽略，不像未知层那样报错（它不改字节语义）
        let header = br#"{"layers":[],"meta":{"mime":"text/plain","future":7}}"#;
        let mut file = Vec::new();
        file.extend_from_slice(MAGIC);
        file.push(FORMAT_VERSION);
        file.extend_from_slice(&(header.len() as u32).to_be_bytes());
        file.extend_from_slice(header);
        file.extend_from_slice(b"payload");

        assert_eq!(inspect(&file).unwrap().meta.mime, "text/plain");
        assert_eq!(decode(&file, &Secrets::default()).unwrap(), b"payload");
    }

    /// gpg 那两层（签名、加密）。这一条以前没有，所以"验签参数传反了"没被发现。
    ///
    /// 需要一个能生成密钥的 gpg：测试用**临时家目录**，不去动用户自己的钥匙串。
    #[test]
    fn gpg_layers_round_trip() {
        let _guard = gpg_guard();
        // 没有 gpg 的机器上，这些功能本来就不可用（那条路径另有用例钉住）
        if !gpg_available() {
            eprintln!("跳过 gpg 测试：这台计算机上没有 gpg");
            return;
        }

        let home = std::env::temp_dir().join(format!("refind-note-gpg-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();

        let recipient = "refind-note-test@example.com";
        prepare_gpg_home(&home, recipient);
        set_gpg_home(home.clone());

        let content = b"sign me".to_vec();

        // 只签名：不改载荷，所以未加密的 blob 不需要口令也能验
        let signed = encode(
            &content,
            &Meta::default(),
            &Policy {
                gpg_sign: Some(recipient.to_string()),
                ..Default::default()
            },
            None,
        )
        .unwrap();

        let protection = inspect(&signed).unwrap().protection;
        assert!(
            protection.sign.is_some(),
            "头是明文，签名状态不该需要钥匙才看得出来"
        );
        assert_eq!(decode(&signed, &Secrets::default()).unwrap(), content);

        // 验签报告：过没过、指纹、以及本地对这枚公钥的信任程度
        let report = signature_report(&signed, &Secrets::default())
            .unwrap()
            .expect("签过的 blob 应当有报告");
        assert!(report.verified, "{report:?}");
        assert!(report.detail.contains("指纹"), "{report:?}");
        assert!(report.trust.is_some(), "{report:?}");

        // 内容被动过一个字节：报告说"没通过"，而不是抛错 —— 验签的结论与验签跑不起来要分开
        let mut broken = signed.clone();
        let last = broken.len() - 1;
        broken[last] ^= 0x01;
        let report = signature_report(&broken, &Secrets::default())
            .unwrap()
            .expect("层链没变，报告照样出得来");
        assert!(!report.verified, "{report:?}");
        assert!(!report.detail.is_empty(), "{report:?}");

        // 没签名层就没有报告
        let plain = encode(&content, &Meta::default(), &Policy::default(), None).unwrap();
        assert!(signature_report(&plain, &Secrets::default())
            .unwrap()
            .is_none());

        // 加密：载荷变了，解出来要一模一样
        let sealed = encode(
            &content,
            &Meta::default(),
            &Policy {
                gpg_encrypt: Some(recipient.to_string()),
                ..Default::default()
            },
            None,
        )
        .unwrap();
        assert!(!sealed
            .windows(content.len())
            .any(|window| window == content));
        assert!(inspect(&sealed).unwrap().protection.encrypt.is_some());
        assert_eq!(decode(&sealed, &Secrets::default()).unwrap(), content);

        // 签名 + 压缩 + 加密，三层一起
        let whole = encode(
            &content,
            &Meta::default(),
            &Policy {
                compress: true,
                gpg_sign: Some(recipient.to_string()),
                gpg_encrypt: Some(recipient.to_string()),
                symmetric: false,
            },
            None,
        )
        .unwrap();
        assert_eq!(decode(&whole, &Secrets::default()).unwrap(), content);

        let _ = std::fs::remove_dir_all(&home);
    }

    /// 信任程度的人话映射。它不碰钥匙串，所以不必等 gpg
    #[test]
    fn trust_labels_say_what_they_mean() {
        assert_eq!(trust_label(gpgme::Validity::Unknown), "本机没有这把公钥");
        assert_eq!(trust_label(gpgme::Validity::Full), "完全信任");
        assert_eq!(trust_label(gpgme::Validity::Ultimate), "绝对信任");
    }

    /// 没有 gpg 时：这些功能**不可用**，给一句说得清的错，而不是底层报错。
    #[test]
    fn without_gpg_the_gpg_features_report_unavailable() {
        let _guard = gpg_guard();

        FORCE_NO_GPG.store(true, std::sync::atomic::Ordering::Relaxed);
        let signed = encode(
            b"x",
            &Meta::default(),
            &Policy {
                gpg_sign: Some("谁".to_string()),
                ..Default::default()
            },
            None,
        );
        let sealed = encode(
            b"x",
            &Meta::default(),
            &Policy {
                gpg_encrypt: Some("谁".to_string()),
                ..Default::default()
            },
            None,
        );
        FORCE_NO_GPG.store(false, std::sync::atomic::Ordering::Relaxed);

        for result in [signed, sealed] {
            let error = result.unwrap_err();
            assert_eq!(error, CodecError::GpgUnavailable);
            assert!(error.to_string().contains("没有 gpg"), "{error}");
        }
    }

    /// 建一个临时的 gpg 家目录，并生成一把**无口令**的测试密钥。
    ///
    /// 测试环境里没有交互，所以只能这么办；没生成出来就直接失败 ——
    /// 静默跳过会让"签名坏了"这件事重新藏起来。
    fn prepare_gpg_home(home: &std::path::Path, user: &str) {
        let status = std::process::Command::new("gpg")
            .arg("--homedir")
            .arg(home)
            .args([
                "--batch",
                "--pinentry-mode",
                "loopback",
                "--passphrase",
                "",
                "--quick-generate-key",
                user,
                "default",
                "default",
                "never",
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();

        match status {
            // 连 gpg 都没装：这条路径无从测起
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                eprintln!("跳过 gpg 测试：找不到 gpg 命令");
            }
            Ok(status) if status.success() => {}
            other => panic!("生成 gpg 测试密钥失败：{other:?}"),
        }
    }

    #[test]
    fn the_address_of_a_signed_blob_changes_with_the_signature() {
        // 签名不改载荷，但签名本身进头 → 落盘字节不同 → 地址不同。
        // 这正是"签过的文件不该被当成同一份"的落实。
        let layers = |signature: &str| {
            vec![Layer::Gpg {
                mode: GpgMode::Sign,
                key: "ABC".to_string(),
                signature: Some(signature.to_string()),
            }]
        };
        let first = assemble(
            &layers("c2lnbmF0dXJlLTE="),
            &Meta::default(),
            b"same payload".to_vec(),
        )
        .unwrap();
        let second = assemble(
            &layers("c2lnbmF0dXJlLTI="),
            &Meta::default(),
            b"same payload".to_vec(),
        )
        .unwrap();

        assert_ne!(first, second);
        assert_ne!(
            crate::storage::store::hash_hex(&first),
            crate::storage::store::hash_hex(&second)
        );
    }
}
