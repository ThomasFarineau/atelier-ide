//! Commit graph of the Git log, drawn as lanes like IntelliJ: one row per commit, no connector
//! rows. Lanes are compacted after every row, so a lane that ends lets the ones on its right move
//! left with a diagonal edge.

/// A line drawn in a row of the graph. Columns are lane indexes; colors are indexes into a
/// palette, stable along the first-parent chain of a branch.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Edge {
    /// A lane passing by the commit, from the top of the row to the bottom.
    Through {
        top: usize,
        bottom: usize,
        color: usize,
    },
    /// A child above, joining the commit from the top of the row.
    ToNode { top: usize, color: usize },
    /// A parent below, reached from the commit to the bottom of the row.
    FromNode { bottom: usize, color: usize },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GraphRow {
    /// Lane of the commit's node.
    pub column: usize,
    pub color: usize,
    pub edges: Vec<Edge>,
    /// Number of lanes used by the row.
    pub width: usize,
}

struct Lane {
    /// Hash of the commit this lane leads to.
    target: String,
    color: usize,
}

/// Lays out commits given in log order (children before parents) with their parent hashes.
pub fn layout<'a>(commits: impl IntoIterator<Item = (&'a str, &'a [String])>) -> Vec<GraphRow> {
    let mut lanes: Vec<Lane> = Vec::new();
    let mut next_color = 0;
    let mut rows = Vec::new();
    for (hash, parents) in commits {
        let matching: Vec<usize> = (0..lanes.len())
            .filter(|&i| lanes[i].target == hash)
            .collect();
        let column = matching.first().copied().unwrap_or(lanes.len());
        let color = match matching.first() {
            Some(&i) => lanes[i].color,
            None => {
                next_color += 1;
                next_color - 1
            }
        };
        let mut edges: Vec<Edge> = matching
            .iter()
            .map(|&i| Edge::ToNode {
                top: i,
                color: lanes[i].color,
            })
            .collect();
        // Lanes below the row; `None` origin marks the lanes leaving the node.
        let mut after: Vec<(Lane, Option<usize>)> = Vec::new();
        let first_parent = parents.first().map(|p| Lane {
            target: p.clone(),
            color,
        });
        let before = std::mem::take(&mut lanes);
        let mut first_parent = first_parent;
        for (i, lane) in before.into_iter().enumerate() {
            if i == column {
                after.extend(first_parent.take().map(|l| (l, None)));
            } else if !matching.contains(&i) {
                after.push((lane, Some(i)));
            }
        }
        // A new branch tip, or a commit whose lane was the last one.
        let node_slot = after
            .iter()
            .take_while(|(_, origin)| origin.is_some_and(|o| o < column))
            .count();
        if let Some(lane) = first_parent.take() {
            after.insert(node_slot, (lane, None));
        }
        let mut insert_at = node_slot + usize::from(!parents.is_empty());
        for parent in parents.iter().skip(1) {
            if let Some(k) = after.iter().position(|(l, _)| &l.target == parent) {
                edges.push(Edge::FromNode {
                    bottom: k,
                    color: after[k].0.color,
                });
                continue;
            }
            after.insert(
                insert_at,
                (
                    Lane {
                        target: parent.clone(),
                        color: next_color,
                    },
                    None,
                ),
            );
            next_color += 1;
            insert_at += 1;
        }
        let mut width = after.len().max(column + 1);
        for (k, (lane, origin)) in after.iter().enumerate() {
            edges.push(match origin {
                Some(top) => {
                    width = width.max(top + 1);
                    Edge::Through {
                        top: *top,
                        bottom: k,
                        color: lane.color,
                    }
                }
                None => Edge::FromNode {
                    bottom: k,
                    color: lane.color,
                },
            });
        }
        for &i in &matching {
            width = width.max(i + 1);
        }
        lanes = after.into_iter().map(|(lane, _)| lane).collect();
        rows.push(GraphRow {
            column,
            color,
            edges,
            width,
        });
    }
    rows
}

#[cfg(test)]
mod tests {
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
}
