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
#[path = "../../../../test/plugins/builtins/git/graph/tests.rs"]
mod tests;
