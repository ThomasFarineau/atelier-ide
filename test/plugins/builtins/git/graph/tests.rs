use super::*;

fn run(commits: &[(&str, &[&str])]) -> Vec<GraphRow> {
    let owned: Vec<(String, Vec<String>)> = commits
        .iter()
        .map(|(h, p)| (h.to_string(), p.iter().map(|s| s.to_string()).collect()))
        .collect();
    layout(owned.iter().map(|(h, p)| (h.as_str(), p.as_slice())))
}

#[test]
fn linear_history_stays_in_one_lane() {
    let rows = run(&[("c", &["b"]), ("b", &["a"]), ("a", &[])]);
    assert!(
        rows.iter()
            .all(|r| r.column == 0 && r.color == 0 && r.width == 1)
    );
    assert_eq!(
        rows[0].edges,
        [Edge::FromNode {
            bottom: 0,
            color: 0
        }]
    );
    assert_eq!(
        rows[1].edges,
        [
            Edge::ToNode { top: 0, color: 0 },
            Edge::FromNode {
                bottom: 0,
                color: 0
            }
        ]
    );
    assert_eq!(rows[2].edges, [Edge::ToNode { top: 0, color: 0 }]);
}

#[test]
fn branch_then_merge() {
    // m merges f into b; f and b both come from a.
    let rows = run(&[("m", &["b", "f"]), ("f", &["a"]), ("b", &["a"]), ("a", &[])]);
    // The merge opens a second lane for the merged branch, in a new color.
    assert_eq!(rows[0].column, 0);
    assert_eq!(rows[0].width, 2);
    assert!(rows[0].edges.contains(&Edge::FromNode {
        bottom: 1,
        color: 1
    }));
    // f sits in the second lane while the first one passes by.
    assert_eq!((rows[1].column, rows[1].color), (1, 1));
    assert!(rows[1].edges.contains(&Edge::Through {
        top: 0,
        bottom: 0,
        color: 0
    }));
    // b keeps the first lane; both lanes then join on a.
    assert_eq!((rows[2].column, rows[2].color), (0, 0));
    assert_eq!(rows[3].column, 0);
    let joins: Vec<_> = rows[3]
        .edges
        .iter()
        .filter(|e| matches!(e, Edge::ToNode { .. }))
        .collect();
    assert_eq!(joins.len(), 2);
    assert_eq!(rows[3].width, 2);
}

#[test]
fn two_tips_get_their_own_lanes_and_colors() {
    let rows = run(&[("x", &["a"]), ("y", &["a"]), ("a", &[])]);
    assert_eq!((rows[0].column, rows[0].color), (0, 0));
    // The second tip has no child above: it starts a new lane on the right.
    assert_eq!((rows[1].column, rows[1].color), (1, 1));
    assert!(
        !rows[1]
            .edges
            .iter()
            .any(|e| matches!(e, Edge::ToNode { .. }))
    );
    assert!(rows[1].edges.contains(&Edge::Through {
        top: 0,
        bottom: 0,
        color: 0
    }));
    assert_eq!(rows[2].column, 0);
    // The lane of y ends on a: nothing goes further down.
    assert!(
        !rows[2]
            .edges
            .iter()
            .any(|e| matches!(e, Edge::FromNode { .. }))
    );
}

#[test]
fn lanes_shift_left_when_one_ends() {
    // Three tips; the middle one ends on a root, the right one moves left.
    let rows = run(&[
        ("x", &["a"]),
        ("y", &["r"]),
        ("z", &["a"]),
        ("r", &[]),
        ("a", &[]),
    ]);
    assert_eq!(rows[2].column, 2);
    assert_eq!(rows[3].column, 1);
    assert!(rows[3].edges.contains(&Edge::Through {
        top: 2,
        bottom: 1,
        color: 2
    }));
}
