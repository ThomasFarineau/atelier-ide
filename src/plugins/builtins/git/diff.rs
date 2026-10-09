//! Line diffs for the diff viewer and the editor gutter.

use crate::plugins::builtin::{LineMark, MarkKind};
use similar::{DiffOp, TextDiff};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RowKind {
    Equal,
    Deleted,
    Inserted,
    /// Old and new line replaced each other (side by side only).
    Changed,
    /// Unchanged lines hidden; the text is empty.
    Fold,
}

/// A line of the diff viewer. Line numbers are 1-based.
#[derive(Clone, Debug)]
pub struct Row {
    pub kind: RowKind,
    pub old: Option<(usize, String)>,
    pub new: Option<(usize, String)>,
    /// Hidden lines of a fold.
    pub hidden: usize,
}

fn normalize(text: &str) -> String {
    text.replace("\r\n", "\n")
}

fn ops(
    old: &str,
    new: &str,
    context: Option<usize>,
) -> (Vec<Vec<DiffOp>>, Vec<String>, Vec<String>) {
    let diff = TextDiff::from_lines(old, new);
    let groups = match context {
        Some(n) => diff.grouped_ops(n),
        None => vec![diff.ops().to_vec()],
    };
    let lines = |t: &str| t.lines().map(str::to_string).collect::<Vec<_>>();
    (groups, lines(old), lines(new))
}

fn fold(hidden: usize) -> Row {
    Row {
        kind: RowKind::Fold,
        old: None,
        new: None,
        hidden,
    }
}

/// Side-by-side rows. With `context`, unchanged lines further than `context` lines from a change
/// are folded.
pub fn side_by_side(old: &str, new: &str, context: Option<usize>) -> Vec<Row> {
    let (old, new) = (normalize(old), normalize(new));
    let (groups, a, b) = ops(&old, &new, context);
    let line =
        |lines: &[String], i: usize| Some((i + 1, lines.get(i).cloned().unwrap_or_default()));
    let mut rows = Vec::new();
    let mut shown_old = 0;
    for group in groups {
        let Some(first) = group.first() else { continue };
        let start = first.old_range().start;
        if context.is_some() && start > shown_old {
            rows.push(fold(start - shown_old));
        }
        for op in &group {
            match *op {
                DiffOp::Equal {
                    old_index,
                    new_index,
                    len,
                } => {
                    rows.extend((0..len).map(|i| Row {
                        kind: RowKind::Equal,
                        old: line(&a, old_index + i),
                        new: line(&b, new_index + i),
                        hidden: 0,
                    }));
                }
                DiffOp::Delete {
                    old_index, old_len, ..
                } => {
                    rows.extend((0..old_len).map(|i| Row {
                        kind: RowKind::Deleted,
                        old: line(&a, old_index + i),
                        new: None,
                        hidden: 0,
                    }));
                }
                DiffOp::Insert {
                    new_index, new_len, ..
                } => {
                    rows.extend((0..new_len).map(|i| Row {
                        kind: RowKind::Inserted,
                        old: None,
                        new: line(&b, new_index + i),
                        hidden: 0,
                    }));
                }
                DiffOp::Replace {
                    old_index,
                    old_len,
                    new_index,
                    new_len,
                } => {
                    rows.extend((0..old_len.max(new_len)).map(|i| Row {
                        kind: RowKind::Changed,
                        old: (i < old_len).then(|| line(&a, old_index + i)).flatten(),
                        new: (i < new_len).then(|| line(&b, new_index + i)).flatten(),
                        hidden: 0,
                    }));
                }
            }
        }
        shown_old = group.last().map_or(shown_old, |op| op.old_range().end);
    }
    if context.is_some() && shown_old < a.len() && !rows.is_empty() {
        rows.push(fold(a.len() - shown_old));
    }
    rows
}

/// Unified rows: deleted lines then inserted lines; `Changed` is never used.
pub fn unified(old: &str, new: &str, context: Option<usize>) -> Vec<Row> {
    let mut rows = Vec::new();
    let mut inserted = Vec::new();
    for row in side_by_side(old, new, context) {
        if row.kind == RowKind::Changed {
            if let Some(old) = row.old {
                rows.push(Row {
                    kind: RowKind::Deleted,
                    old: Some(old),
                    new: None,
                    hidden: 0,
                });
            }
            if let Some(new) = row.new {
                inserted.push(Row {
                    kind: RowKind::Inserted,
                    old: None,
                    new: Some(new),
                    hidden: 0,
                });
            }
        } else {
            rows.append(&mut inserted);
            rows.push(row);
        }
    }
    rows.append(&mut inserted);
    rows
}

/// Index of the first row of each change block.
pub fn change_starts(rows: &[Row]) -> Vec<usize> {
    let changed = |r: &Row| !matches!(r.kind, RowKind::Equal | RowKind::Fold);
    (0..rows.len())
        .filter(|&i| changed(&rows[i]) && (i == 0 || !changed(&rows[i - 1])))
        .collect()
}

/// Gutter markers of `new` compared with `old`.
pub fn line_marks(old: &str, new: &str) -> Vec<LineMark> {
    let (old, new) = (normalize(old), normalize(new));
    let diff = TextDiff::from_lines(&old, &new);
    let mut marks = Vec::new();
    for op in diff.ops() {
        let (kind, range) = match *op {
            DiffOp::Equal { .. } => continue,
            DiffOp::Delete { new_index, .. } => {
                marks.push(LineMark {
                    line: new_index,
                    kind: MarkKind::Deleted,
                });
                continue;
            }
            DiffOp::Insert {
                new_index, new_len, ..
            } => (MarkKind::Added, new_index..new_index + new_len),
            DiffOp::Replace {
                new_index, new_len, ..
            } => (MarkKind::Modified, new_index..new_index + new_len),
        };
        marks.extend(range.map(|line| LineMark { line, kind }));
    }
    marks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn side_by_side_pairs_replaced_lines() {
        let rows = side_by_side("a\nb\nc\n", "a\nB\nc\nd\n", None);
        let kinds: Vec<_> = rows.iter().map(|r| r.kind).collect();
        assert_eq!(
            kinds,
            [
                RowKind::Equal,
                RowKind::Changed,
                RowKind::Equal,
                RowKind::Inserted
            ]
        );
        assert_eq!(rows[1].old, Some((2, "b".into())));
        assert_eq!(rows[1].new, Some((2, "B".into())));
        assert_eq!(rows[3].new, Some((4, "d".into())));
        assert_eq!(change_starts(&rows), [1, 3]);
    }

    #[test]
    fn unified_lists_deletions_before_insertions() {
        let rows = unified("x\ny\n", "X\nY\n", None);
        let kinds: Vec<_> = rows.iter().map(|r| r.kind).collect();
        assert_eq!(
            kinds,
            [
                RowKind::Deleted,
                RowKind::Deleted,
                RowKind::Inserted,
                RowKind::Inserted
            ]
        );
    }

    #[test]
    fn unchanged_lines_are_folded() {
        let old: String = (1..=20).map(|i| format!("{i}\n")).collect();
        let new = old.replace("10\n", "dix\n");
        let rows = side_by_side(&old, &new, Some(2));
        assert_eq!(rows.first().unwrap().kind, RowKind::Fold);
        assert_eq!(rows.first().unwrap().hidden, 7);
        assert_eq!(rows.last().unwrap().hidden, 8);
        assert_eq!(rows.len(), 7);
    }

    #[test]
    fn gutter_marks_ignore_line_endings() {
        let marks = line_marks("a\r\nb\r\nc\r\n", "a\nB\nc\nnew\n");
        assert_eq!(
            marks,
            [
                LineMark {
                    line: 1,
                    kind: MarkKind::Modified
                },
                LineMark {
                    line: 3,
                    kind: MarkKind::Added
                },
            ]
        );
        let marks = line_marks("a\nb\n", "a\n");
        assert_eq!(
            marks,
            [LineMark {
                line: 1,
                kind: MarkKind::Deleted
            }]
        );
    }
}
