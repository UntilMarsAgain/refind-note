//! 压缩层：deflate 与 brotli 两档。
//!
//! 档位范围不一样（deflate 0–9，brotli 0–11），所以由层里记着的 `level` 说话，
//! **不看当前设置** —— 这是"读只照头"那条规矩在这一层的落实。
//!
//! 为什么它排在最内层：外面一旦加密，信息熵已经拉满，再压只会让体积略微变大
//! 还多烧一遍 CPU。

use std::io::Write;

use flate2::write::{DeflateDecoder, DeflateEncoder};
use flate2::Compression as DeflateLevel;

use super::types::Compression;
use super::{CodecError, Result};

/// 压：两档算法的档位范围不同（deflate 0–9，brotli 0–11），由层里记着的 `level` 说话
pub(super) fn compress_with(algorithm: Compression, content: &[u8], level: u32) -> Result<Vec<u8>> {
    match algorithm {
        Compression::Deflate => {
            let mut encoder = DeflateEncoder::new(Vec::new(), DeflateLevel::new(level));
            encoder
                .write_all(content)
                .map_err(|error| CodecError::Corrupt(format!("压缩失败：{error}")))?;
            encoder
                .finish()
                .map_err(|error| CodecError::Corrupt(format!("压缩失败：{error}")))
        }
        Compression::Brotli => {
            let mut out = Vec::new();
            {
                // 窗口 22 位（4 MiB）：对笔记这种体量足够，也不必再调
                let mut writer = brotli::CompressorWriter::new(&mut out, 4096, level, 22);
                writer
                    .write_all(content)
                    .map_err(|error| CodecError::Corrupt(format!("压缩失败：{error}")))?;
            }
            Ok(out)
        }
    }
}

/// 解：**照头上写的那一档**，不看当前设置
pub(super) fn decompress_with(algorithm: Compression, payload: &[u8]) -> Result<Vec<u8>> {
    match algorithm {
        Compression::Deflate => {
            let mut decoder = DeflateDecoder::new(Vec::new());
            decoder
                .write_all(payload)
                .map_err(|error| CodecError::Corrupt(format!("解压失败：{error}")))?;
            decoder
                .finish()
                .map_err(|error| CodecError::Corrupt(format!("解压失败：{error}")))
        }
        Compression::Brotli => {
            let mut out = Vec::new();
            let mut reader = brotli::Decompressor::new(payload, 4096);
            std::io::copy(&mut reader, &mut out)
                .map_err(|error| CodecError::Corrupt(format!("解压失败：{error}")))?;
            Ok(out)
        }
    }
}
