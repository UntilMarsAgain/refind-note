#!/usr/bin/env python3
"""把增量保存接进读写路径。

规则（来自需求）：
1. 增量不见得更小 —— 把补丁的实际字节数与完整内容真比一次，谁小用谁；
2. 增量可以递归依赖增量，但链太长读取会慢 —— 超过 delta_chain_limit 退回整份快照；
3. 提交的基准只能**是提交**（提交不能依赖草稿）；草稿按当前决定一律整份存；
4. 二进制（非文本）一律整份存。
"""
import pathlib

def patch(path, pairs, must=None):
    p = pathlib.Path(path)
    t = p.read_text()
    for old, new in pairs:
        assert t.count(old) == 1, f"{path}：锚点出现 {t.count(old)} 次 -> {old[:60]!r}"
        t = t.replace(old, new, 1)
    if must:
        for needle in must:
            assert needle in t, f"{path}：缺少 {needle}"
    p.write_text(t)
    print(f"  {path} ok")

print("改数据类型：")
error_already = "Corrupt" in pathlib.Path("src-tauri/src/storage/error.rs").read_text()
if error_already:
    print("  error.rs 已改过，跳过")
else:
    patch("src-tauri/src/storage/error.rs", [
      ("""    /// 改名时目标标题已经被别的笔记占用
    NameTaken(String),""",
     """    /// 改名时目标标题已经被别的笔记占用
    NameTaken(String),
    /// 数据本身坏了：补丁与基准对不上、增量链断裂等
    Corrupt(String),"""),
    ("""            Self::NameTaken(t) => write!(f, "《{t}》已经存在，换一个名字"),""",
     """            Self::NameTaken(t) => write!(f, "《{t}》已经存在，换一个名字"),
            Self::Corrupt(why) => write!(f, "数据损坏：{why}"),"""),
], must=["Corrupt"])

# ---------------- event.rs：事件带上「怎么存的」
p = pathlib.Path("src-tauri/src/storage/event.rs")
t = p.read_text()

t = t.replace("""    /// 一次提交
    Rev {
        at: String,
        rev: u64,
        blob: String,
        bytes: u64,
        mime: String,""",
"""    /// 一次提交
    Rev {
        at: String,
        rev: u64,
        /// 内容落在哪个 blob 上：整份就是正文，增量就是补丁
        blob: String,
        bytes: u64,
        /// `full`（整份）或 `delta`（相对 `base_rev` 的补丁）
        #[serde(default = "default_encoding")]
        encoding: String,
        /// 增量的基准版本号。**一定是提交**——提交不能依赖草稿
        #[serde(default, skip_serializing_if = "Option::is_none")]
        base_rev: Option<u64>,
        /// 这一版完整内容的哈希（增量时也记，便于判断内容是否变过）
        #[serde(default, skip_serializing_if = "String::is_empty")]
        content_hash: String,
        mime: String,""", 1)

t = t.replace("""    /// 自动保存的草稿：也占一个版本号，挂在 `on` 所指的提交上
    Auto {
        at: String,
        rev: u64,
        blob: String,
        bytes: u64,
        mime: String,
        on: u64,
    },""",
"""    /// 自动保存的草稿：也占一个版本号，挂在 `on` 所指的提交上。
    ///
    /// 草稿一律**整份存**：它们随时会被提交取代、被清理，增量省下的那点字节不值得
    /// 让清理逻辑去照顾增量链。（规则 3 允许草稿依赖草稿，这里作为备用能力保留。）
    Auto {
        at: String,
        rev: u64,
        blob: String,
        bytes: u64,
        #[serde(default = "default_encoding")]
        encoding: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        base_rev: Option<u64>,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        content_hash: String,
        mime: String,
        on: u64,
    },""", 1)

t = t.replace("""impl Event {""",
"""/// 老日志没写 `encoding` 时按整份内容理解
fn default_encoding() -> String {
    "full".to_string()
}

impl Event {""", 1)

# NoteState 也带上这几个字段（改名要沿用上一版的存法）
t = t.replace("""    pub blob: Option<String>,
    pub bytes: u64,
    pub mime: String,""",
"""    pub blob: Option<String>,
    pub bytes: u64,
    /// 上一版的存法：改名那一版要原样沿用，否则内容引用会错位
    pub encoding: String,
    pub base_rev: Option<u64>,
    pub content_hash: String,
    pub mime: String,""", 1)

t = t.replace("""            Event::Rev {
                at,
                rev,
                blob,
                bytes,
                mime,
                supersedes,""",
"""            Event::Rev {
                at,
                rev,
                blob,
                bytes,
                encoding,
                base_rev,
                content_hash,
                mime,
                supersedes,""", 1)

t = t.replace("""                state.blob = Some(blob.clone());
                state.bytes = *bytes;
                state.mime = mime.clone();""",
"""                state.blob = Some(blob.clone());
                state.bytes = *bytes;
                state.encoding = encoding.clone();
                state.base_rev = *base_rev;
                state.content_hash = content_hash.clone();
                state.mime = mime.clone();""", 1)

t = t.replace("""            Event::Auto {
                at, bytes, blob, on, ..
            } => {
                state.bytes = *bytes;""",
"""            Event::Auto {
                at,
                bytes,
                blob,
                encoding,
                content_hash,
                on,
                ..
            } => {
                state.bytes = *bytes;
                state.encoding = encoding.clone();
                state.content_hash = content_hash.clone();""", 1)

t = t.replace("""    let mut state = NoteState {
        mime: DEFAULT_MIME.to_string(),
        ..Default::default()
    };""",
"""    let mut state = NoteState {
        mime: DEFAULT_MIME.to_string(),
        encoding: "full".to_string(),
        ..Default::default()
    };""", 1)

for needle in ["encoding", "content_hash", "fn default_encoding"]:
    assert needle in t, needle
p.write_text(t)
print("  event.rs ok")

# ---------------- config.rs：增量链长度上限
patch("src-tauri/src/storage/config.rs", [
    ("""    pub max_title_bytes: usize,
}""",
     """    pub max_title_bytes: usize,
    /// 增量链的长度上限：超过就让下一版退回整份快照，免得读取时一路回放。
    pub delta_chain_limit: usize,
}"""),
    ("""            max_title_bytes: crate::title::MAX_TITLE_BYTES,
        }""",
     """            max_title_bytes: crate::title::MAX_TITLE_BYTES,
            delta_chain_limit: 32,
        }"""),
], must=["delta_chain_limit"])

# ---------------- api.rs：历史里能看出这一版是整份还是增量
patch("src-tauri/src/storage/api.rs", [
    ("""    /// 相对上一条记录的字节增减，便于一眼看出改了多少
    pub delta: i64,""",
     """    /// 相对上一条记录的字节增减，便于一眼看出改了多少
    pub delta: i64,
    /// `full` 或 `delta`：这一版内容是怎么存的
    pub encoding: String,"""),
], must=["pub encoding"])
print("数据类型改完")
