//! 行级差异，供「对比版本」用。
//!
//! 放在后端做有两个好处：前端保持薄（不必引 diff 库、也不必在浏览器里算），
//! 而且差异算法能被单元测试直接钉住。
//!
//! 输出的是**不分组**的逐行序列（equal / insert / delete），长笔记里成片的
//! equal 由前端折叠——这样后端不必了解「上下文留几行」这类展示偏好。

use super::api::{DiffLine, DiffResult};
use similar::{ChangeTag, TextDiff};

/// 比较两段文本，产出可逐行渲染的差异
pub fn line_diff(
    from_rev: u64,
    from_title: &str,
    from: &str,
    to_rev: u64,
    to_title: &str,
    to: &str,
) -> DiffResult {
    let diff = TextDiff::from_lines(from, to);
    let mut lines = Vec::new();
    let mut inserted = 0usize;
    let mut deleted = 0usize;

    for change in diff.iter_all_changes() {
        let kind = match change.tag() {
            ChangeTag::Equal => "equal",
            ChangeTag::Insert => {
                inserted += 1;
                "insert"
            }
            ChangeTag::Delete => {
                deleted += 1;
                "delete"
            }
        };

        lines.push(DiffLine {
            kind: kind.to_string(),
            // 行号 1 起；该侧没有这一行时是 None（旧文件行号对新插入的行为 None）
            old_line: change.old_index().map(|index| index as u64 + 1),
            new_line: change.new_index().map(|index| index as u64 + 1),
            // 行切分会把换行符带进来，去掉；否则前端每一行都会多出一行空白
            text: change
                .value()
                .trim_end_matches(|ch| ch == '\n' || ch == '\r')
                .to_string(),
        });
    }

    DiffResult {
        from_rev,
        to_rev,
        from_title: from_title.to_string(),
        to_title: to_title.to_string(),
        lines,
        inserted,
        deleted,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(result: &DiffResult) -> Vec<&str> {
        result.lines.iter().map(|line| line.kind.as_str()).collect()
    }

    #[test]
    fn identical_text_has_no_changes() {
        let result = line_diff(1, "甲", "一\n二\n三\n", 2, "甲", "一\n二\n三\n");
        assert_eq!(result.inserted, 0);
        assert_eq!(result.deleted, 0);
        assert!(kinds(&result).iter().all(|kind| *kind == "equal"));
    }

    #[test]
    fn changed_line_shows_as_delete_plus_insert() {
        let result = line_diff(1, "甲", "一\n二\n三\n", 2, "甲", "一\n改了\n三\n");
        assert_eq!(result.deleted, 1);
        assert_eq!(result.inserted, 1);
        assert_eq!(kinds(&result), vec!["equal", "delete", "insert", "equal"]);
    }

    #[test]
    fn line_numbers_come_from_each_side() {
        let result = line_diff(1, "甲", "一\n二\n", 2, "甲", "一\n新\n二\n");
        let inserted = result
            .lines
            .iter()
            .find(|line| line.kind == "insert")
            .expect("应当有插入行");
        // 插入的行只存在于新文件那一侧
        assert_eq!(inserted.new_line, Some(2));
        assert_eq!(inserted.old_line, None);
        assert_eq!(inserted.text, "新");
    }

    #[test]
    fn trailing_newline_does_not_create_an_empty_line() {
        let result = line_diff(1, "甲", "只有一行\n", 2, "甲", "只有一行\n");
        assert_eq!(result.lines.len(), 1, "不该多出一个空行：{:?}", result.lines);
        assert_eq!(result.lines[0].text, "只有一行");
    }

    #[test]
    fn empty_to_content_is_all_insert() {
        let result = line_diff(1, "甲", "", 2, "甲", "一\n二\n");
        assert_eq!(result.deleted, 0);
        assert_eq!(result.inserted, 2);
    }
}
