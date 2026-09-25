//! 增量（差异）编码。
//!
//! 一条版本的内容可以整份存，也可以只存「相对某个基准的补丁」。这里只负责
//! **编码与解码**；选哪种存、基准选谁由上层（storage::mod）按规则决定。
//!
//! 为什么不用现成的补丁格式（unified diff）：那东西要解析文本、还要容忍上下文
//! 匹配失败，出错时容易**静默产出错内容**。这里的 ops 是显式的行数指令，解码
//! 要么精确还原、要么明确报错。
//!
//! 落盘格式刻意做得紧凑（一行一个操作），因为**补丁要和完整内容比大小**，
//! JSON 那种每个操作都带字段名的写法会让小改动反而变大：
//!
//! ```text
//! k3          保留 3 行
//! d2          删除 2 行
//! a文本       插入一行（`\` 转义成 `\\`，换行转义成 `\n`）
//! ```
//!
//! 行的切分与 `similar` 保持一致：每行**带**行尾换行符（最后一行可能没有），
//! 这样拼接回去是逐字节还原的。

use similar::{ChangeTag, TextDiff};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeltaOp {
    /// 从基准原样保留 n 行
    Keep { lines: usize },
    /// 从基准跳过 n 行（等于删掉）
    Drop { lines: usize },
    /// 插入这些行
    Add { lines: Vec<String> },
}

#[derive(Debug)]
pub enum DeltaError {
    /// 补丁与基准对不上（行数不匹配），说明链子坏了
    Mismatch { base_lines: usize, consumed: usize },
    /// 补丁本身读不出来
    Corrupt(String),
}

impl std::fmt::Display for DeltaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Mismatch {
                base_lines,
                consumed,
            } => write!(
                f,
                "增量补丁与基准对不上：基准 {base_lines} 行，补丁用掉 {consumed} 行"
            ),
            Self::Corrupt(why) => write!(f, "增量补丁读不出来：{why}"),
        }
    }
}

impl std::error::Error for DeltaError {}

// ---------------------------------------------------------------- 编码

/// 把 `target` 相对 `base` 的差异编码成 ops
pub fn encode(base: &str, target: &str) -> Vec<DeltaOp> {
    let diff = TextDiff::from_lines(base, target);
    let mut ops: Vec<DeltaOp> = Vec::new();
    let mut keep = 0usize;
    let mut drop = 0usize;
    let mut add: Vec<String> = Vec::new();

    // 落 op 的顺序必须是「先保留/删除、再插入」：同一个位置上替换 = 删掉再加
    let flush =
        |ops: &mut Vec<DeltaOp>, keep: &mut usize, drop: &mut usize, add: &mut Vec<String>| {
            if *keep > 0 {
                ops.push(DeltaOp::Keep { lines: *keep });
                *keep = 0;
            }
            if *drop > 0 {
                ops.push(DeltaOp::Drop { lines: *drop });
                *drop = 0;
            }
            if !add.is_empty() {
                ops.push(DeltaOp::Add {
                    lines: std::mem::take(add),
                });
            }
        };

    for change in diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Equal => {
                // 先落掉攒着的删除与插入，否则顺序会乱
                if !add.is_empty() || drop > 0 {
                    flush(&mut ops, &mut keep, &mut drop, &mut add);
                }
                keep += 1;
            }
            ChangeTag::Delete => {
                if !add.is_empty() {
                    flush(&mut ops, &mut keep, &mut drop, &mut add);
                }
                drop += 1;
            }
            ChangeTag::Insert => {
                if drop > 0 {
                    flush(&mut ops, &mut keep, &mut drop, &mut add);
                }
                add.push(change.value().to_string());
            }
        }
    }
    flush(&mut ops, &mut keep, &mut drop, &mut add);

    ops
}

/// 用 ops 从基准还原出目标内容
pub fn apply(base: &str, ops: &[DeltaOp]) -> Result<String, DeltaError> {
    let lines: Vec<&str> = base.split_inclusive('\n').collect();
    let mut cursor = 0usize;
    let mut out = String::with_capacity(base.len());

    for op in ops {
        match op {
            DeltaOp::Keep { lines: count } => {
                for _ in 0..*count {
                    let line = lines.get(cursor).ok_or(DeltaError::Mismatch {
                        base_lines: lines.len(),
                        consumed: cursor + 1,
                    })?;
                    out.push_str(line);
                    cursor += 1;
                }
            }
            DeltaOp::Drop { lines: count } => {
                if cursor + count > lines.len() {
                    return Err(DeltaError::Mismatch {
                        base_lines: lines.len(),
                        consumed: cursor + count,
                    });
                }
                cursor += count;
            }
            DeltaOp::Add { lines: added } => {
                for line in added {
                    out.push_str(line);
                }
            }
        }
    }

    // 基准必须被恰好用完，否则说明这个补丁不是针对这个基准编的
    if cursor != lines.len() {
        return Err(DeltaError::Mismatch {
            base_lines: lines.len(),
            consumed: cursor,
        });
    }

    Ok(out)
}

// ---------------------------------------------------------------- 落盘格式

/// 编码成落盘用的字节（上层拿它的长度跟完整内容比大小）
pub fn encode_bytes(ops: &[DeltaOp]) -> Vec<u8> {
    let mut out = String::new();

    for op in ops {
        match op {
            DeltaOp::Keep { lines } => {
                out.push('k');
                out.push_str(&lines.to_string());
                out.push('\n');
            }
            DeltaOp::Drop { lines } => {
                out.push('d');
                out.push_str(&lines.to_string());
                out.push('\n');
            }
            DeltaOp::Add { lines } => {
                for line in lines {
                    out.push('a');
                    for ch in line.chars() {
                        match ch {
                            '\\' => out.push_str("\\\\"),
                            '\n' => out.push_str("\\n"),
                            other => out.push(other),
                        }
                    }
                    out.push('\n');
                }
            }
        }
    }

    out.into_bytes()
}

/// 从落盘字节解回 ops
pub fn decode_bytes(bytes: &[u8]) -> Result<Vec<DeltaOp>, DeltaError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| DeltaError::Corrupt("不是合法的 UTF-8".to_string()))?;

    let mut ops: Vec<DeltaOp> = Vec::new();

    for line in text.split_inclusive('\n') {
        let body = line.strip_suffix('\n').unwrap_or(line);
        if body.is_empty() {
            continue;
        }

        let mut chars = body.chars();
        let tag = chars.next().unwrap_or('?');
        let rest: String = chars.collect();

        match tag {
            'k' | 'd' => {
                let count: usize = rest
                    .parse()
                    .map_err(|_| DeltaError::Corrupt(format!("行数不是数字：{rest:?}")))?;
                if count == 0 {
                    continue;
                }
                ops.push(if tag == 'k' {
                    DeltaOp::Keep { lines: count }
                } else {
                    DeltaOp::Drop { lines: count }
                });
            }
            'a' => {
                let mut value = String::with_capacity(rest.len());
                let mut escaped = false;
                for ch in rest.chars() {
                    if escaped {
                        value.push(if ch == 'n' { '\n' } else { '\\' });
                        escaped = false;
                    } else if ch == '\\' {
                        escaped = true;
                    } else {
                        value.push(ch);
                    }
                }
                if escaped {
                    value.push('\\');
                }

                // 连续的行插入合并成一个 op，进一步省字节（也与 encode 的形状一致）
                match ops.last_mut() {
                    Some(DeltaOp::Add { lines }) => lines.push(value),
                    _ => ops.push(DeltaOp::Add { lines: vec![value] }),
                }
            }
            other => {
                return Err(DeltaError::Corrupt(format!("未知操作：{other:?}")));
            }
        }
    }

    Ok(ops)
}

/// 补丁落盘时的实际字节数
pub fn encoded_len(ops: &[DeltaOp]) -> usize {
    encode_bytes(ops).len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(base: &str, target: &str) {
        let ops = encode(base, target);
        let restored = apply(base, &ops).expect("补丁应当能还原");
        assert_eq!(restored, target, "base={base:?} target={target:?}");

        // 落盘再读回来也必须还原（转义最容易在这里出错）
        let bytes = encode_bytes(&ops);
        let decoded = decode_bytes(&bytes).expect("落盘格式应当能读回来");
        let restored_again = apply(base, &decoded).expect("读回来的补丁也要能还原");
        assert_eq!(restored_again, target, "落盘往返后不一致");
    }

    #[test]
    fn identical_content_roundtrips() {
        roundtrip("一\n二\n三\n", "一\n二\n三\n");
        let ops = encode("一样\n", "一样\n");
        assert!(ops.iter().all(|op| matches!(op, DeltaOp::Keep { .. })));
    }

    #[test]
    fn single_line_change_roundtrips() {
        roundtrip("一\n二\n三\n", "一\n改了\n三\n");
    }

    #[test]
    fn insertions_and_deletions_roundtrip() {
        roundtrip("一\n三\n", "一\n二\n三\n");
        roundtrip("一\n二\n三\n", "一\n三\n");
        roundtrip("一\n", "一\n二\n三\n四\n");
        roundtrip("一\n二\n三\n四\n", "一\n");
    }

    #[test]
    fn missing_trailing_newline_roundtrips() {
        // 最后一行没有换行符时也必须逐字节还原
        roundtrip("一\n二", "一\n二改");
        roundtrip("一\n二\n", "一\n二");
        roundtrip("", "第一行");
        roundtrip("第一行", "");
    }

    #[test]
    fn content_with_backslashes_roundtrips() {
        // 转义逻辑必须能处理文本里本来就有的反斜杠
        roundtrip("a\\b\n", "a\\b\\c\n");
        roundtrip("路径 C:\\Users\n", "路径 C:\\Users\\名字\n");
        roundtrip("结尾是反斜杠\\", "结尾是反斜杠\\n");
    }

    #[test]
    fn whole_file_replacement_roundtrips() {
        roundtrip("旧\n内容\n", "完全\n不一样\n的东西\n");
    }

    #[test]
    fn tiny_edit_is_smaller_than_a_full_copy() {
        let base = "第一行\n第二行\n第三行\n第四行\n第五行\n";
        let target = "第一行\n第二行\n改过的第三行\n第四行\n第五行\n";
        let ops = encode(base, target);
        assert!(
            encoded_len(&ops) < target.len(),
            "小改动时补丁应当比完整内容小：补丁 {} 字节 / 全文 {} 字节",
            encoded_len(&ops),
            target.len()
        );
    }

    /// 规则一的依据：改动很大时补丁**可能比完整内容还大**（新内容要整段塞进去，
    /// 还多出每个操作的前缀）。所以上层必须真的比过再决定用哪种。
    #[test]
    fn heavy_rewrite_can_be_bigger_than_a_full_copy() {
        let base = "甲\n";
        let mut target = String::new();
        for index in 0..200 {
            target.push_str(&format!("全新的第 {index} 行内容\n"));
        }
        let ops = encode(base, &target);
        assert!(
            encoded_len(&ops) > target.len(),
            "大改动的补丁 {} 字节，确实比全文 {} 字节大",
            encoded_len(&ops),
            target.len()
        );
    }

    #[test]
    fn applying_to_the_wrong_base_is_rejected() {
        let ops = encode("一\n二\n三\n", "一\n二\n");
        let error = apply("完全不同的基准\n", &ops).expect_err("基准不对就该报错");
        assert!(matches!(error, DeltaError::Mismatch { .. }));
    }

    #[test]
    fn corrupt_patch_is_rejected() {
        assert!(matches!(
            decode_bytes(b"x1\n"),
            Err(DeltaError::Corrupt(_))
        ));
        assert!(matches!(
            decode_bytes(b"knotanumber\n"),
            Err(DeltaError::Corrupt(_))
        ));
        assert!(matches!(
            decode_bytes(&[0xff, 0xfe]),
            Err(DeltaError::Corrupt(_))
        ));
    }
}
