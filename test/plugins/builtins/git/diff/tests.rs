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
