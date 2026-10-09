//! Questions put to the graph, answered in a few lines instead of the files themselves: what in
//! the project matches some words, what a name is connected to, how two things are linked. Each
//! answer stops when its budget (in characters, about four per token) is spent.

use super::{build::Graph, report::describe};
use std::collections::{HashMap, HashSet, VecDeque};

const STOPWORDS: [&str; 30] = [
    "le",
    "la",
    "les",
    "un",
    "une",
    "des",
    "du",
    "de",
    "et",
    "ou",
    "est",
    "dans",
    "pour",
    "que",
    "qui",
    "sur",
    "the",
    "and",
    "how",
    "what",
    "where",
    "does",
    "this",
    "that",
    "with",
    "for",
    "are",
    "comment",
    "quoi",
    "fonctionne",
];

/// The words of a text, lowercased, with `camelCase` and `snake_case` taken apart.
fn words(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut previous_lower = false;
    for c in text.chars() {
        if !c.is_alphanumeric() {
            if !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
            previous_lower = false;
            continue;
        }
        if c.is_uppercase() && previous_lower && !current.is_empty() {
            out.push(std::mem::take(&mut current));
        }
        previous_lower = c.is_lowercase();
        current.extend(c.to_lowercase());
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// The links of each node: (link index, true when the node is the source).
fn adjacency(graph: &Graph) -> Vec<Vec<(usize, bool)>> {
    let mut adjacent = vec![Vec::new(); graph.nodes.len()];
    for (i, link) in graph.links.iter().enumerate() {
        adjacent[link.source].push((i, true));
        adjacent[link.target].push((i, false));
    }
    adjacent
}

/// A relation seen from a node: `appelle`, `appelé par`, `importe`…
fn relation(kind: &str, outgoing: bool) -> &'static str {
    match (kind, outgoing) {
        ("calls", true) => "calls",
        ("calls", false) => "called by",
        ("imports", true) => "imports",
        ("imports", false) => "imported by",
        ("contains", true) => "contains",
        ("contains", false) => "defined in",
        ("extends", true) => "extends",
        ("extends", false) => "extended by",
        ("implements", true) => "implements",
        ("implements", false) => "implemented by",
        ("instantiates", true) => "instantiates",
        ("instantiates", false) => "instantiated by",
        ("declares", true) => "declares module",
        ("declares", false) => "module declared by",
        ("links", true) => "links to",
        _ => "referenced by",
    }
}

/// The relations of a node as one line each, the most useful first.
fn relations(graph: &Graph, adjacent: &[(usize, bool)], per_kind: usize) -> Vec<String> {
    let mut groups: Vec<(&'static str, Vec<String>)> = Vec::new();
    for &(i, outgoing) in adjacent {
        let link = &graph.links[i];
        let other = if outgoing { link.target } else { link.source };
        let name = relation(&link.kind, outgoing);
        let label = format!(
            "{}{}",
            graph.nodes[other].label,
            if link.confidence == "INFERRED" {
                "?"
            } else {
                ""
            }
        );
        match groups.iter_mut().find(|(n, _)| *n == name) {
            Some((_, list)) => list.push(label),
            None => groups.push((name, vec![label])),
        }
    }
    groups
        .into_iter()
        .map(|(name, list)| {
            let more = list.len().saturating_sub(per_kind);
            format!(
                "{name}: {}{}",
                list.into_iter()
                    .take(per_kind)
                    .collect::<Vec<_>>()
                    .join(", "),
                if more > 0 {
                    format!(" (+{more})")
                } else {
                    String::new()
                }
            )
        })
        .collect()
}

/// What matches `question`: the nodes whose names (or files) share its words, with what they are
/// linked to.
pub fn query(graph: &Graph, question: &str, budget: usize) -> String {
    let tokens: Vec<String> = words(question)
        .into_iter()
        .filter(|w| w.chars().count() >= 3 && !STOPWORDS.contains(&w.as_str()))
        .collect();
    if tokens.is_empty() {
        return "Give at least one word of the code (a function, class or file name).".into();
    }
    let mut scored: Vec<(u32, usize)> = Vec::new();
    for (i, node) in graph.nodes.iter().enumerate() {
        let label_words = words(&node.label);
        let label = node.label.to_lowercase();
        let file = node.file.to_lowercase();
        let mut score = 0;
        for token in &tokens {
            if label == *token || label.rsplit("::").next() == Some(token.as_str()) {
                score += 10;
            } else if label_words.contains(token) {
                score += 6;
            } else if label.contains(token.as_str()) {
                score += 3;
            } else if node.kind != "file" && file.contains(token.as_str()) {
                score += 1;
            } else if node.kind == "file" && file.contains(token.as_str()) {
                score += 2;
            }
        }
        if score > 0 {
            scored.push((score, i));
        }
    }
    scored.sort_by_key(|&(score, i)| {
        (
            std::cmp::Reverse(score),
            std::cmp::Reverse(graph.nodes[i].degree),
            i,
        )
    });
    if scored.is_empty() {
        return format!("Nothing in the graph matches “{question}”.");
    }
    let adjacent = adjacency(graph);
    let mut out = format!("Elements of the project related to “{question}”:\n\n");
    let total = scored.len();
    let mut shown = 0;
    for (_, node) in scored {
        let mut entry = format!(
            "- {} · group {}\n",
            describe(graph, node),
            graph
                .communities
                .get(graph.nodes[node].community)
                .map_or("?", |c| c.label.as_str())
        );
        for line in relations(graph, &adjacent[node], 8) {
            entry.push_str(&format!("    {line}\n"));
        }
        if out.len() + entry.len() > budget && shown > 0 {
            break;
        }
        out.push_str(&entry);
        shown += 1;
    }
    if shown < total {
        out.push_str(&format!(
            "\n… {} more results: refine the question or raise the budget.\n",
            total - shown
        ));
    }
    out
}

/// The nodes a name stands for: by exact name first, then by the end of a path or of a name.
fn find(graph: &Graph, name: &str) -> Vec<usize> {
    let wanted = name.trim().to_lowercase();
    let exact: Vec<usize> = (0..graph.nodes.len())
        .filter(|&i| graph.nodes[i].label.to_lowercase() == wanted)
        .collect();
    if !exact.is_empty() {
        return exact;
    }
    (0..graph.nodes.len())
        .filter(|&i| {
            let label = graph.nodes[i].label.to_lowercase();
            label.ends_with(&format!("::{wanted}")) || label.ends_with(&format!("/{wanted}"))
        })
        .collect()
}

/// Everything the graph knows of a name.
pub fn node(graph: &Graph, name: &str, budget: usize) -> String {
    let found = find(graph, name);
    if found.is_empty() {
        return format!(
            "No element named “{name}” in the graph. Try graph_query with a few words."
        );
    }
    let adjacent = adjacency(graph);
    let mut out = String::new();
    for &i in found.iter().take(5) {
        let n = &graph.nodes[i];
        out.push_str(&format!(
            "{}\n  group: {} · links: {}\n",
            describe(graph, i),
            graph
                .communities
                .get(n.community)
                .map_or("?", |c| c.label.as_str()),
            n.degree
        ));
        for line in relations(graph, &adjacent[i], 15) {
            out.push_str(&format!("  {line}\n"));
        }
        out.push('\n');
        if out.len() > budget {
            break;
        }
    }
    if found.len() > 5 {
        out.push_str(&format!(
            "… and {} more elements of that name.\n",
            found.len() - 5
        ));
    }
    out
}

/// The shortest chain of links between two names.
pub fn path(graph: &Graph, from: &str, to: &str) -> String {
    let (starts, goals) = (find(graph, from), find(graph, to));
    if starts.is_empty() || goals.is_empty() {
        return format!(
            "“{}” is not in the graph.",
            if starts.is_empty() { from } else { to }
        );
    }
    let adjacent = adjacency(graph);
    let goal: HashSet<usize> = goals.iter().copied().collect();
    let mut came: HashMap<usize, (usize, usize, bool)> = HashMap::new();
    let mut queue: VecDeque<usize> = starts.iter().copied().collect();
    let mut seen: HashSet<usize> = starts.iter().copied().collect();
    let mut reached = None;
    while let Some(current) = queue.pop_front() {
        if goal.contains(&current) {
            reached = Some(current);
            break;
        }
        for &(i, outgoing) in &adjacent[current] {
            let link = &graph.links[i];
            let next = if outgoing { link.target } else { link.source };
            if seen.insert(next) {
                came.insert(next, (current, i, outgoing));
                queue.push_back(next);
            }
        }
    }
    let Some(end) = reached else {
        return format!("No path between “{from}” and “{to}” in the graph.");
    };
    let mut steps = Vec::new();
    let mut at = end;
    while let Some(&(previous, link, outgoing)) = came.get(&at) {
        steps.push((previous, link, outgoing, at));
        at = previous;
    }
    steps.reverse();
    let mut out = format!("{}\n", describe(graph, at));
    for (previous, link, outgoing, next) in steps {
        let _ = previous;
        out.push_str(&format!(
            "  → {} {}\n",
            relation(&graph.links[link].kind, outgoing),
            describe(graph, next)
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{build::build, extract::extract};
    use std::{collections::BTreeMap, path::Path};

    fn graph() -> Graph {
        // One folder per call: the tests run side by side and each removes its own.
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("atelier-graph-query-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src")).unwrap();
        let sources = [
            (
                "src/math.js",
                "export function addNumbers(a, b) { return a + b; }\n",
            ),
            (
                "src/app.js",
                "import { addNumbers } from './math.js';\nexport function computeTotal(x) { return addNumbers(x, 1); }\n",
            ),
            (
                "src/view.js",
                "import { computeTotal } from './app.js';\nexport function renderTotal() { return computeTotal(2); }\n",
            ),
            ("src/unrelated.js", "export function standalone() {}\n"),
        ];
        let mut facts = BTreeMap::new();
        for (rel, text) in sources {
            std::fs::write(root.join(rel), text).unwrap();
            facts.insert(rel.to_string(), extract("js", text));
        }
        let graph = build(&root, &facts, 0);
        let _ = std::fs::remove_dir_all(&root);
        let _ = Path::new("");
        graph
    }

    #[test]
    fn words_are_taken_apart_by_case_and_separators() {
        assert_eq!(
            words("computeTotal_value-2"),
            ["compute", "total", "value", "2"]
        );
    }

    #[test]
    fn a_question_finds_the_matching_elements_with_their_relations() {
        let graph = graph();
        let answer = query(&graph, "where is the total computed?", 4000);
        assert!(
            answer.contains("computeTotal") && answer.contains("renderTotal"),
            "{answer}"
        );
        assert!(
            answer.contains("calls: addNumbers") || answer.contains("called by: renderTotal"),
            "{answer}"
        );
        assert!(!answer.contains("standalone"), "{answer}");
        assert!(query(&graph, "blablabla", 4000).starts_with("Nothing in the graph"));
        assert!(
            query(&graph, "the and for", 4000).contains("at least one word"),
            "only stop words"
        );
    }

    #[test]
    fn the_budget_cuts_the_answer_and_says_what_is_left() {
        let graph = graph();
        let answer = query(&graph, "total add compute render", 200);
        assert!(answer.contains("more results"), "{answer}");
        assert!(answer.len() < 700, "{} chars", answer.len());
    }

    #[test]
    fn a_node_lists_what_it_is_connected_to_and_a_path_links_two_names() {
        let graph = graph();
        let info = node(&graph, "computeTotal", 4000);
        assert!(
            info.contains("calls: addNumbers") && info.contains("called by: renderTotal"),
            "{info}"
        );
        assert!(node(&graph, "n'existe pas", 4000).starts_with("No element named"));
        let route = path(&graph, "renderTotal", "addNumbers");
        assert!(
            route.contains("renderTotal")
                && route.contains("computeTotal")
                && route.contains("addNumbers"),
            "{route}"
        );
        assert!(path(&graph, "standalone", "addNumbers").starts_with("No path between"));
    }
}
