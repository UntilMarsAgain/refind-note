//! 封装格式里的**类型**：算法、层链、明文头与那几份自述。
//!
//! 这一层只描述"文件长什么样"，一个字节都不碰 —— 读写的动作在
//! [`super`]，各层的具体算法分别在 [`super::compress`] / [`super::symmetric`] /
//! [`super::gpg`]。
//!
//! 算法名与层 id 是**格式的一部分**：它们原样写进明文头，改名字等于改格式，
//! 已经落盘的读不出来。所以 [`KNOWN_LAYERS`](super::KNOWN_LAYERS) 与各枚举的
//! serde 名都只增不改。

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------- 算法
//
// 算法名是**写进头里的**，所以它们是**格式的一部分**：改名字就是改格式，
// 会让已经落盘的那些读不出来。加算法就往后面加，别动已有的。

/// 压缩算法
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Compression {
    /// zlib/deflate：快，通用
    #[default]
    Deflate,
    /// brotli：更小、更慢（静态内容划算）
    Brotli,
}

impl Compression {
    /// 界面上写给人看的名字
    pub fn label(self) -> &'static str {
        match self {
            Self::Deflate => "deflate",
            Self::Brotli => "brotli",
        }
    }

    /// 这一档的默认参数（deflate 是 0–9 的档，brotli 是 0–11）
    pub(super) fn default_level(self) -> u32 {
        match self {
            Self::Deflate => 6,
            Self::Brotli => 5,
        }
    }
}

/// 口令层的对称加密算法（口令派生都是 Argon2id，参数记在头里）。
///
/// 两档都是 AEAD：自带完整性校验，改一个字节都解不开。
///
/// **名字是写死的**（不是 `rename_all`）：它要跟着界面上的写法走，而
/// `rename_all = "kebab-case"` 会把 `Aes256Gcm` 拼成 `aes256-gcm` —— 少一个横线，
/// 界面那边写的是 `aes-256-gcm`，两边对不上就是一句
/// "unknown variant `aes-256-gcm`"。别名收着旧写法：早先存下来的
/// `preferences.json` / `sync.json` 里是 `aes256-gcm`，读得回来。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Cipher {
    /// AES-256-GCM：国际通用的那一档，有硬件指令的机器上很快
    #[default]
    #[serde(rename = "aes-256-gcm", alias = "aes256-gcm")]
    Aes256Gcm,
    /// SM4-GCM：国密（GB/T 32907 的 SM4 配上 GCM 那套认证）
    #[serde(rename = "sm4-gcm", alias = "sm4gcm")]
    Sm4Gcm,
}

impl Cipher {
    /// 界面上写给人看的名字
    pub fn label(self) -> &'static str {
        match self {
            Self::Aes256Gcm => "aes-256-gcm",
            Self::Sm4Gcm => "sm4-gcm",
        }
    }

    /// 这一档的密钥长度（SM4 的密钥是 128 位，与 AES-256 的 256 位不同）
    pub(super) fn key_bytes(self) -> usize {
        match self {
            Self::Aes256Gcm => 32,
            Self::Sm4Gcm => 16,
        }
    }
}

// ---------------------------------------------------------------- 层

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "layer", rename_all = "kebab-case")]
pub enum Layer {
    /// 压缩：算法与档位都写在头里（**读的时候照它**，不看当前设置）
    Compress {
        algorithm: Compression,
        /// 档位：deflate 是 0–9，brotli 是 0–11
        level: u32,
    },
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
        /// 用哪种算法
        cipher: Cipher,
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
    /// 这一把在钥匙串里的主用户标识（`姓名 <邮箱>`）；本机没有这把钥匙就是 `None`
    ///
    /// 指纹是**唯一**的标识，但人认不出它 —— 界面上要写得出"这是谁签的"。
    pub uid: Option<String>,
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
    /// 这一把在钥匙串里的主用户标识（`姓名 <邮箱>`）；本机没有这把钥匙就是 `None`
    pub uid: Option<String>,
    /// 本机有没有对应的**私钥** —— 有才解得开
    pub secret: bool,
    /// 人话说明
    pub detail: String,
}

/// 一个 blob 声明用了哪些层 —— **明文头里就有，不需要口令**
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Protection {
    pub compress: bool,
    /// 压缩用的算法（这一版头上记着的；没压过时没有意义）
    pub compression: Compression,
    /// 签名者密钥指纹
    pub sign: Option<String>,
    /// 加密到的密钥指纹
    pub encrypt: Option<String>,
    /// 是否套了口令对称层
    pub symmetric: bool,
    /// 口令层用的算法（没套口令层时没有意义）
    pub cipher: Cipher,
}

impl Protection {
    /// 什么都没套：落盘的就是明文
    ///
    /// 有**两处**要它：还没有正文的那一版（0 版）压根没有封装头，
    /// 以及"连头都读不出来"的场合（那时只能 conservatively 当成没加密 ——
    /// 宁可让人点一下让 gpg 去问，也不要摆一个骗人的口令输入框）。
    pub fn plain() -> Self {
        Self {
            compress: false,
            compression: Compression::default(),
            sign: None,
            encrypt: None,
            symmetric: false,
            cipher: Cipher::default(),
        }
    }

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
    /// 压缩用哪种算法（`compress` 为假时这一栏没意义）
    pub compression: Compression,
    /// 用哪把密钥签名；`None` = 不签
    pub gpg_sign: Option<String>,
    /// 用哪把密钥加密；`None` = 不加密
    pub gpg_encrypt: Option<String>,
    /// 要不要再套一层口令对称加密
    pub symmetric: bool,
    /// 口令层用哪种算法（`symmetric` 为假时这一栏没意义）
    pub cipher: Cipher,
}

/// 解开某些层需要的秘密
#[derive(Default)]
pub struct Secrets<'a> {
    /// 对称层的口令
    pub passphrase: Option<&'a str>,
}
