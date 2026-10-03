//! 口令对称层：Argon2id 派生 + AEAD 加解密。
//!
//! 它是**最外**的一层，也是唯一一层需要人输口令的（见 [`super::Secrets`]）。
//!
//! 派生参数（m/t/p 代价）与 salt、nonce 都记在明文头里，所以换一台机器、换一个
//! 密码强度都能照原样解开；而**算法名也记在头里**，于是同一份格式容得下好几档算法。
//!
//! 口令不对与密文被改过在这里是同一件事：AEAD 分不出来，也不该分。

use aes_gcm::aead::{Aead, KeyInit};
use argon2::{Algorithm, Argon2, Params, Version};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;

use super::types::{Cipher, Layer};
use super::{
    CodecError, Result, DEFAULT_M_COST, DEFAULT_P_COST, DEFAULT_T_COST, KEY_BYTES, NONCE_BYTES,
    SALT_BYTES,
};

/// AES-256-GCM
type Aes256Gcm = aes_gcm::Aes256Gcm;
/// SM4-GCM：同一个 GCM 实现，只是把分组密码换成 SM4（国密）
type Sm4Gcm = aes_gcm::AesGcm<sm4::Sm4, aes_gcm::aead::consts::U12>;

pub(super) fn symmetric_seal(
    content: &[u8],
    passphrase: &str,
    cipher_kind: Cipher,
) -> Result<(Vec<u8>, Layer)> {
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
    let sealed = seal_with(cipher_kind, &key, &nonce_bytes, content)?;

    Ok((
        sealed,
        Layer::Symmetric {
            cipher: cipher_kind,
            salt: BASE64.encode(salt),
            nonce: BASE64.encode(nonce_bytes),
            m_cost: DEFAULT_M_COST,
            t_cost: DEFAULT_T_COST,
            p_cost: DEFAULT_P_COST,
        },
    ))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn symmetric_open(
    payload: &[u8],
    passphrase: &str,
    cipher_kind: Cipher,
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
    open_with(cipher_kind, &key, &nonce, payload)
}

/// 用这一档算法封上。两档都是 AEAD，nonce 都是 12 字节。
pub(super) fn seal_with(cipher_kind: Cipher, key: &[u8], nonce: &[u8], content: &[u8]) -> Result<Vec<u8>> {
    // SM4 的密钥是 128 位：从派生出来的 32 字节里取前 16 字节（不是"截短了强度"，
    // 这一档算法本来就用这么长的钥匙）
    let key = &key[..cipher_kind.key_bytes()];
    let nonce = aes_gcm::Nonce::<aes_gcm::aead::consts::U12>::try_from(nonce)
        .map_err(|_| CodecError::Corrupt("nonce 长度不对".to_string()))?;

    match cipher_kind {
        Cipher::Aes256Gcm => Aes256Gcm::new_from_slice(key)
            .map_err(|error| CodecError::Corrupt(format!("密钥长度不对：{error}")))?
            .encrypt(&nonce, content)
            .map_err(|_| CodecError::WrongPassphrase),
        Cipher::Sm4Gcm => Sm4Gcm::new_from_slice(key)
            .map_err(|error| CodecError::Corrupt(format!("密钥长度不对：{error}")))?
            .encrypt(&nonce, content)
            .map_err(|_| CodecError::WrongPassphrase),
    }
}

/// 照头里那一档算法解开
pub(super) fn open_with(cipher_kind: Cipher, key: &[u8], nonce: &[u8], payload: &[u8]) -> Result<Vec<u8>> {
    let key = &key[..cipher_kind.key_bytes()];
    let nonce = aes_gcm::Nonce::<aes_gcm::aead::consts::U12>::try_from(nonce)
        .map_err(|_| CodecError::Corrupt("nonce 长度不对".to_string()))?;

    let opened = match cipher_kind {
        Cipher::Aes256Gcm => Aes256Gcm::new_from_slice(key)
            .map_err(|error| CodecError::Corrupt(format!("密钥长度不对：{error}")))?
            .decrypt(&nonce, payload),
        Cipher::Sm4Gcm => Sm4Gcm::new_from_slice(key)
            .map_err(|error| CodecError::Corrupt(format!("密钥长度不对：{error}")))?
            .decrypt(&nonce, payload),
    };
    // 口令不对与密文被改过在这里是同一件事：AEAD 分不出来，也不该分
    opened.map_err(|_| CodecError::WrongPassphrase)
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
