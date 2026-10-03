//! **云端那一层**：每一份传上去 / 取回来的字节都从这里过一道，再交给引擎。
//!
//! 这一层的意义在 [`CloudCipher`] 的文档里（防的是"存储服务与捡到那个桶的人"，与笔记
//! 自身那几层 gpg / 口令不重叠）。它实现的是 [`SyncTransform`] —— 引擎只认这个接口，
//! 于是"要不要封、用哪一档算法"是这一层的事，编排那一层不必知道自己运的是什么。
//!
//! 同一节里还放三样给界面看的东西：结果 [`SyncReport`]（含一次取舍 [`Conflict`]）与进度
//! [`Progress`]。它们同样是"这一趟"的形状，而不是引擎的细节。

use serde::Serialize;

/// 上传前后给字节过一道的钩子。
///
/// 现在只有"原样"这一种实现（[`Plaintext`]）与封一层（[`CloudCipher`]）两种，但接口把
/// **位置**先留出来了：将来要加别的实现，传进 [`crate::features::sync::run`] 就行 —— 引擎
/// 不必知道自己运的是什么，也就不会因为"怎么做的"而改动（同一份独立性，与"路径该不该
/// 同步"交给工作目录那一层回答是一个道理）。
pub trait SyncTransform: Send + Sync {
    /// 上传之前：本机的字节 → 传上去的字节
    fn seal(&self, relative: &str, bytes: Vec<u8>) -> Result<Vec<u8>, String>;
    /// 下载之后：云端的字节 → 本机的字节
    fn open(&self, relative: &str, bytes: Vec<u8>) -> Result<Vec<u8>, String>;
}

/// 原样（不加密）——默认就是这个
pub struct Plaintext;

impl SyncTransform for Plaintext {
    fn seal(&self, _relative: &str, bytes: Vec<u8>) -> Result<Vec<u8>, String> {
        Ok(bytes)
    }
    fn open(&self, _relative: &str, bytes: Vec<u8>) -> Result<Vec<u8>, String> {
        Ok(bytes)
    }
}

/// 传上去的每一份都过这一层：**整份仓库**因此一起被保护起来 ——
/// 日志、标题表、命名空间表，以及那些没单独加密的笔记，都在内。
///
/// 与笔记自身那几层（gpg / 口令）是**两件事**：那几层防的是"拿到你这台机器的人"，
/// 这一层防的是"存储服务、以及捡到那个桶的人"。两者不重叠，各管各的。
///
/// 封出来的东西自带说明（见 [`CloudEnvelope`]）：没有这一层的老对象（明文传上去的）
/// 读的时候原样放行 —— 换了设置不至于把已经传上去的东西读废。
pub struct CloudCipher {
    key: [u8; 32],
    cipher: crate::storage::codec::Cipher,
}

/// 云端的封装：`RNDS` + 版本 + 这一档算法 + nonce + 密文。
///
/// 头上写着用哪一档算法，是为了"以后换了算法还读得回旧的"——与 blob 那边同一个道理。
pub(super) struct CloudEnvelope;

impl CloudEnvelope {
    pub(super) const MAGIC: &'static [u8; 4] = b"RNDS";
    const VERSION: u8 = 1;
    /// magic(4) + version(1) + cipher(1) + nonce(12)
    const HEADER: usize = 18;

    /// 这一串字节是不是我们封出来的
    pub(super) fn matches(bytes: &[u8]) -> bool {
        bytes.len() >= Self::HEADER && &bytes[..4] == Self::MAGIC && bytes[4] == Self::VERSION
    }

    fn cipher_byte(cipher: crate::storage::codec::Cipher) -> u8 {
        use crate::storage::codec::Cipher;
        match cipher {
            Cipher::Aes256Gcm => 1,
            Cipher::Sm4Gcm => 2,
        }
    }

    fn cipher_of(byte: u8) -> Option<crate::storage::codec::Cipher> {
        use crate::storage::codec::Cipher;
        match byte {
            1 => Some(Cipher::Aes256Gcm),
            2 => Some(Cipher::Sm4Gcm),
            _ => None,
        }
    }
}

impl CloudCipher {
    /// 从 base64 那把钥匙做出来；空串（没设）返回 `Err`
    pub fn from_key(key: &str) -> Result<Self, String> {
        use base64::Engine as _;
        let raw = base64::engine::general_purpose::STANDARD
            .decode(key.trim())
            .map_err(|_| "云端密钥不是一串 base64，抄的时候可能少了几个字".to_string())?;
        if raw.len() != 32 {
            return Err(format!("云端密钥应该是 32 字节，这把是 {} 字节", raw.len()));
        }
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&raw);
        Ok(Self {
            key: bytes,
            // 一开始按默认那一档；真正用哪一档由调用方说了算
            // （同步那边是 `SyncSettings::cipher` 里的设置，见 `with_cipher`）
            cipher: crate::storage::codec::Cipher::default(),
        })
    }

    /// 换一档算法（同步层与笔记那边用同一档，界面上不单独问）
    pub fn with_cipher(mut self, cipher: crate::storage::codec::Cipher) -> Self {
        self.cipher = cipher;
        self
    }
}

impl SyncTransform for CloudCipher {
    fn seal(&self, relative: &str, bytes: Vec<u8>) -> Result<Vec<u8>, String> {
        // 每个对象一条独立的 nonce：同一份内容传两次，密文也不一样
        let mut nonce = [0u8; 12];
        getrandom::getrandom(&mut nonce).map_err(|error| format!("取随机数失败：{error}"))?;
        let sealed = seal_bytes(self.cipher, &self.key, &nonce, &bytes)
            .map_err(|error| format!("加密 {relative} 失败：{error}"))?;

        let mut out = Vec::with_capacity(CloudEnvelope::HEADER + sealed.len());
        out.extend_from_slice(CloudEnvelope::MAGIC);
        out.push(CloudEnvelope::VERSION);
        out.push(CloudEnvelope::cipher_byte(self.cipher));
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&sealed);
        Ok(out)
    }

    fn open(&self, relative: &str, bytes: Vec<u8>) -> Result<Vec<u8>, String> {
        // 不是我们封的（没开加密时传上去的老对象）：原样放行
        if !CloudEnvelope::matches(&bytes) {
            return Ok(bytes);
        }
        let cipher = CloudEnvelope::cipher_of(bytes[5])
            .ok_or_else(|| format!("{relative} 用了本程序不认识的加密档"))?;
        let nonce = &bytes[6..CloudEnvelope::HEADER];
        open_bytes(cipher, &self.key, nonce, &bytes[CloudEnvelope::HEADER..])
            .map_err(|error| format!("解开 {relative} 失败（密钥对不对？）：{error}"))
    }
}

/// 用这一档算法封上（与笔记那边同一套 AEAD，只是头不同 —— 这里要的是"自带说明"）
fn seal_bytes(
    cipher: crate::storage::codec::Cipher,
    key: &[u8; 32],
    nonce: &[u8],
    content: &[u8],
) -> Result<Vec<u8>, String> {
    use crate::storage::codec::Cipher;
    use aes_gcm::aead::{Aead, KeyInit};

    match cipher {
        Cipher::Aes256Gcm => {
            let cipher = aes_gcm::Aes256Gcm::new_from_slice(key)
                .map_err(|error| format!("密钥长度不对：{error}"))?;
            let nonce =
                aes_gcm::Nonce::try_from(nonce).map_err(|_| "nonce 长度不对".to_string())?;
            cipher
                .encrypt(&nonce, content)
                .map_err(|_| "加密失败".to_string())
        }
        Cipher::Sm4Gcm => {
            let cipher =
                aes_gcm::AesGcm::<sm4::Sm4, aes_gcm::aead::consts::U12>::new_from_slice(&key[..16])
                    .map_err(|error| format!("密钥长度不对：{error}"))?;
            let nonce =
                aes_gcm::Nonce::try_from(nonce).map_err(|_| "nonce 长度不对".to_string())?;
            cipher
                .encrypt(&nonce, content)
                .map_err(|_| "加密失败".to_string())
        }
    }
}

/// 照头里那一档算法解开
fn open_bytes(
    cipher_kind: crate::storage::codec::Cipher,
    key: &[u8; 32],
    nonce: &[u8],
    payload: &[u8],
) -> Result<Vec<u8>, String> {
    use crate::storage::codec::Cipher;
    use aes_gcm::aead::{Aead, KeyInit};

    match cipher_kind {
        Cipher::Aes256Gcm => {
            let cipher = aes_gcm::Aes256Gcm::new_from_slice(key)
                .map_err(|error| format!("密钥长度不对：{error}"))?;
            let nonce =
                aes_gcm::Nonce::try_from(nonce).map_err(|_| "nonce 长度不对".to_string())?;
            cipher
                .decrypt(&nonce, payload)
                .map_err(|_| "口令不对，或者内容被改过".to_string())
        }
        Cipher::Sm4Gcm => {
            let cipher =
                aes_gcm::AesGcm::<sm4::Sm4, aes_gcm::aead::consts::U12>::new_from_slice(&key[..16])
                    .map_err(|error| format!("密钥长度不对：{error}"))?;
            let nonce =
                aes_gcm::Nonce::try_from(nonce).map_err(|_| "nonce 长度不对".to_string())?;
            cipher
                .decrypt(&nonce, payload)
                .map_err(|_| "口令不对，或者内容被改过".to_string())
        }
    }
}

/// 一次同步的结果（给界面看）
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct SyncReport {
    pub uploaded: usize,
    pub downloaded: usize,
    /// 云端跟着删掉的
    pub removed_remote: usize,
    /// 本机跟着删掉的（别的机器删过）
    pub removed_local: usize,
    /// 两边都改过、按时间取了新版的那些（路径 + 取了哪边）
    pub conflicts: Vec<Conflict>,
    pub bytes_up: u64,
    pub bytes_down: u64,
}

/// 一次"两边都改过"的取舍
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Conflict {
    pub path: String,
    /// 取了哪一边：`local` / `remote`
    pub kept: String,
}

/// 跑到哪儿了（界面据此写进度）
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Progress {
    /// `lock` / `upload` / `download` / `done`
    pub phase: String,
    pub done: usize,
    pub total: usize,
    /// 一句人话（界面上直接显示）
    pub text: String,
}