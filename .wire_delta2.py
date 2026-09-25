#!/usr/bin/env python3
"""增量保存接线：mod.rs 的写入侧与读取侧"""
import pathlib

p = pathlib.Path("src-tauri/src/storage/mod.rs")
t = p.read_text()

# ---------------- 新增：内容存储（选存法）与按版本取内容
helpers = '''    // ------------------------------------------------------------ 内容存储
    //
    // 一版内容可以整份存，也可以只存「相对某个基准的补丁」。三条规则：
    // 1. 增量**不见得更小**（大改动要把新内容整段塞进去），所以真比一次再决定；
    // 2. 增量可以递归依赖增量，但链太长读取会慢 —— 超过上限就退回整份快照；
    // 3. 提交的基准**只能是提交**（不能依赖草稿），草稿一律整份存；
    // 4. 二进制（非文本）一律整份存。

    /// 某个版本所在的增量链有多长（从最近一次整份快照算起）
    fn chain_length(&self, events: &[Event], rev: u64) -> usize {
        let mut length = 0usize;
        let mut cursor = Some(rev);

        while let Some(current) = cursor {
            let Some(event) = events.iter().find(|event| match event {
                Event::Rev { rev, .. } | Event::Auto { rev, .. } => *rev == current,
                _ => false,
            }) else {
                break;
            };

            let (encoding, base) = match event {
                Event::Rev {
                    encoding, base_rev, ..
                }
                | Event::Auto {
                    encoding, base_rev, ..
                } => (encoding.as_str(), *base_rev),
                _ => break,
            };

            if encoding != "delta" {
                break;
            }
            length += 1;
            cursor = base;
        }

        length
    }

    /// 决定这一版怎么存：整份快照，还是相对基准的增量
    fn store_content(
        &self,
        events: &[Event],
        base: Option<(u64, String)>,
        mime: &str,
        content: &str,
    ) -> Result<Stored, VaultError> {
        let content_hash = hash_bytes(content.as_bytes());

        if mime == DEFAULT_MIME {
            if let Some((base_rev, base_text)) = base {
                if self.chain_length(events, base_rev) < self.config.delta_chain_limit {
                    let ops = delta::encode(&base_text, content);
                    let patch = delta::encode_bytes(&ops);

                    // 规则一：跟完整内容真比一次，补丁更大就用整份
                    if patch.len() < content.len() {
                        let blob = self.blobs.put(&patch)?;
                        return Ok(Stored {
                            blob,
                            encoding: "delta",
                            base_rev: Some(base_rev),
                            content_hash,
                        });
                    }
                }
            }
        }

        let blob = self.blobs.put(content.as_bytes())?;
        Ok(Stored {
            blob,
            encoding: "full",
            base_rev: None,
            content_hash,
        })
    }

    /// 取某个版本的内容：整份直接读，增量先取基准再套补丁
    fn content_of(&self, events: &[Event], rev: u64, depth: usize) -> Result<String, VaultError> {
        if depth > MAX_DELTA_DEPTH {
            return Err(VaultError::Corrupt(format!(
                "增量链超过 {MAX_DELTA_DEPTH} 层，可能已经损坏"
            )));
        }

        let found = events.iter().find_map(|event| match event {
            Event::Rev {
                rev: item_rev,
                blob,
                encoding,
                base_rev,
                ..
            }
            | Event::Auto {
                rev: item_rev,
                blob,
                encoding,
                base_rev,
                ..
            } if *item_rev == rev => Some((blob.clone(), encoding.clone(), *base_rev)),
            _ => None,
        });

        let Some((blob, encoding, base_rev)) = found else {
            return Err(VaultError::Corrupt(format!("找不到版本 {rev} 的内容")));
        };

        let bytes = self.blobs.get(&blob)?;
        if encoding != "delta" {
            return String::from_utf8(bytes)
                .map_err(|_| VaultError::NotText(format!("版本 {rev}")));
        }

        let base_rev = base_rev
            .ok_or_else(|| VaultError::Corrupt(format!("版本 {rev} 标为增量却没有基准")))?;
        let base_text = self.content_of(events, base_rev, depth + 1)?;
        let ops = delta::decode_bytes(&bytes)
            .map_err(|error| VaultError::Corrupt(error.to_string()))?;
        delta::apply(&base_text, &ops).map_err(|error| VaultError::Corrupt(error.to_string()))
    }

'''

anchor = "    // ------------------------------------------------------------ 读"
assert t.count(anchor) == 1
t = t.replace(anchor, helpers + anchor, 1)

# Stored 结构
t = t.replace("""pub struct Vault {""", """/// 一次写入的结果：选了哪种存法、落在哪个 blob 上
struct Stored {
    blob: String,
    encoding: &'static str,
    base_rev: Option<u64>,
    content_hash: String,
}

/// 读取时允许的最大增量链深度（写入侧已有上限，这里是防损坏的兜底）
const MAX_DELTA_DEPTH: usize = 1024;

pub struct Vault {""", 1)

# ---------------- commit：用 store_content
old = """        let blob = self.blobs.put(markdown_text.as_bytes())?;
        let rev = next_rev(&events);
        self.append(
            &id,
            &Event::Rev {
                at: now_iso(),
                rev,
                blob,
                bytes: markdown_text.len() as u64,
                mime: DEFAULT_MIME.to_string(),"""
new = """        // 增量基准取上一个**提交**的内容（绝不用草稿：草稿会被清理）
        let base = if state.rev > 0 {
            Some((state.rev, self.content_of(&events, state.rev, 0)?))
        } else {
            None
        };
        let stored = self.store_content(&events, base, DEFAULT_MIME, markdown_text)?;

        let rev = next_rev(&events);
        self.append(
            &id,
            &Event::Rev {
                at: now_iso(),
                rev,
                blob: stored.blob,
                bytes: markdown_text.len() as u64,
                encoding: stored.encoding.to_string(),
                base_rev: stored.base_rev,
                content_hash: stored.content_hash,
                mime: DEFAULT_MIME.to_string(),"""
assert t.count(old) == 1
t = t.replace(old, new, 1)

# ---------------- rename：沿用上一版的存法
old = """                blob: state.blob.clone().unwrap_or_default(),
                bytes: state.bytes,
                mime: state.mime.clone(),"""
new = """                blob: state.blob.clone().unwrap_or_default(),
                bytes: state.bytes,
                // 内容没变，存法也照旧沿用
                encoding: state.encoding.clone(),
                base_rev: state.base_rev,
                content_hash: state.content_hash.clone(),
                mime: state.mime.clone(),"""
assert t.count(old) == 1
t = t.replace(old, new, 1)

# ---------------- save_draft：草稿一律整份存
old = """        let blob = hash_bytes(markdown_text.as_bytes());
        // 内容没变就不追加，免得自动保存把日志灌满
        if let Some(draft) = &state.draft {
            if draft.blob == blob {
                return Ok(());
            }
        }

        let rev = next_rev(&events);
        self.blobs.put(markdown_text.as_bytes())?;
        self.append(
            &id,
            &Event::Auto {
                at: now_iso(),
                rev,
                blob,
                bytes: markdown_text.len() as u64,
                mime: DEFAULT_MIME.to_string(),
                on: base_rev,
            },
        )"""
new = """        let content_hash = hash_bytes(markdown_text.as_bytes());
        // 内容没变就不追加，免得自动保存把日志灌满
        if let Some(draft) = &state.draft {
            if draft.blob == content_hash {
                return Ok(());
            }
        }

        let rev = next_rev(&events);
        let blob = self.blobs.put(markdown_text.as_bytes())?;
        self.append(
            &id,
            &Event::Auto {
                at: now_iso(),
                rev,
                blob,
                bytes: markdown_text.len() as u64,
                // 草稿整份存：它们随时会被提交取代、被清理，不值得为增量链操心
                encoding: "full".to_string(),
                base_rev: None,
                content_hash,
                mime: DEFAULT_MIME.to_string(),
                on: base_rev,
            },
        )"""
assert t.count(old) == 1
t = t.replace(old, new, 1)

# ---------------- load_outcome：按版本回放
old = """        let markdown_text = match &state.blob {
            Some(blob) => String::from_utf8(self.blobs.get(blob)?)
                .map_err(|_| VaultError::NotText(display.clone()))?,
            None => String::new(),
        };"""
new = """        // rev 0 是「建了但还没提交」，内容为空；否则按版本回放（可能是增量）
        let markdown_text = if state.rev == 0 {
            String::new()
        } else {
            self.content_of(&events, state.rev, 0)?
        };"""
assert t.count(old) == 1
t = t.replace(old, new, 1)

# ---------------- revision：同样按版本回放
old = """        let markdown_text = String::from_utf8(self.blobs.get(&blob)?)
            .map_err(|_| VaultError::NotText(parsed.title.clone()))?;"""
new = """        let markdown_text = self.content_of(&events, rev, 0)?;"""
assert t.count(old) == 1
t = t.replace(old, new, 1)

# ---------------- history：把存法带出来
old = """            let (rev, kind, at, bytes, supersedes, summary) = match event {
                Event::Meta { at, .. } => (0, "create", at.clone(), 0u64, Vec::new(), None),
                Event::Rev {
                    at,
                    rev,
                    bytes,
                    supersedes,
                    summary,
                    ..
                } => (
                    *rev,
                    "commit",
                    at.clone(),
                    *bytes,
                    supersedes.clone(),
                    summary.clone(),
                ),
                Event::Auto {
                    at, rev, bytes, on, ..
                } => (
                    *rev,
                    "draft",
                    at.clone(),
                    *bytes,
                    Vec::new(),
                    Some(format!("自动保存（基于版本 {on}）")),
                ),
                Event::Del { at, rev, summary } => (
                    *rev,
                    "delete",
                    at.clone(),
                    0u64,
                    Vec::new(),
                    summary.clone(),
                ),
            };"""
new = """            let (rev, kind, at, bytes, supersedes, summary, encoding) = match event {
                Event::Meta { at, .. } => {
                    (0, "create", at.clone(), 0u64, Vec::new(), None, "full")
                }
                Event::Rev {
                    at,
                    rev,
                    bytes,
                    encoding,
                    supersedes,
                    summary,
                    ..
                } => (
                    *rev,
                    "commit",
                    at.clone(),
                    *bytes,
                    supersedes.clone(),
                    summary.clone(),
                    encoding.as_str(),
                ),
                Event::Auto {
                    at,
                    rev,
                    bytes,
                    encoding,
                    on,
                    ..
                } => (
                    *rev,
                    "draft",
                    at.clone(),
                    *bytes,
                    Vec::new(),
                    Some(format!("自动保存（基于版本 {on}）")),
                    encoding.as_str(),
                ),
                Event::Del { at, rev, summary } => (
                    *rev,
                    "delete",
                    at.clone(),
                    0u64,
                    Vec::new(),
                    summary.clone(),
                    "full",
                ),
            };"""
assert t.count(old) == 1
t = t.replace(old, new, 1)

old = """                bytes,
                delta,
                supersedes,
                summary,
            });"""
new = """                bytes,
                delta,
                encoding: encoding.to_string(),
                supersedes,
                summary,
            });"""
assert t.count(old) == 1
t = t.replace(old, new, 1)

for needle in ["fn store_content", "fn content_of", "fn chain_length", "struct Stored"]:
    assert needle in t, needle
p.write_text(t)
print("mod.rs ok", len(t.splitlines()), "行")
