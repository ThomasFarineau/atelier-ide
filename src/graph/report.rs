//! The graph as a short Markdown report an AI reads in place of the files, and as a JSON file in
//! the node-link form most graph tools read.

use super::build::Graph;
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn clock(ms: u64) -> String {
    crate::plugins::builtins::mcp::clock(ms)
}

/// `name [kind] (file:line)`
pub fn describe(graph: &Graph, node: usize) -> String {
    let n = &graph.nodes[node];
    if n.kind == "file" {
        format!("{} (file)", n.label)
    } else {
        format!("{} [{}] ({}:{})", n.label, n.kind, n.file, n.line)
    }
}

pub fn symbols(graph: &Graph) -> usize {
    graph.nodes.iter().filter(|n| n.kind != "file").count()
}

pub fn files(graph: &Graph) -> usize {
    graph.nodes.iter().filter(|n| n.kind == "file").count()
}

/// Characters per token, as a rule of thumb for code and English text.
const CHARS_PER_TOKEN: u64 = 4;
/// The symbols the saving is measured on.
const SAMPLE: usize = 20;
/// The budget a question to the graph is given in the benchmark, as the MCP tools give it.
const ANSWER_BUDGET: usize = 6000;

/// `1234` as `1.2k`, `2_500_000` as `2.5M`.
fn tokens(n: u64) -> String {
    match n {
        0..=999 => n.to_string(),
        1_000..=999_999 => format!("{:.1}k", n as f64 / 1e3),
        _ => format!("{:.1}M", n as f64 / 1e6),
    }
}

/// What an AI saves on a question about the project: how many tokens the graph answers with, on
/// average, against how many it takes to read what it would otherwise open.
#[derive(Debug, PartialEq, Eq)]
pub struct Benchmark {
    pub questions: usize,
    /// Average tokens of an answer of the graph.
    pub graph: u64,
    /// Average tokens of the files that define the symbol and its direct neighbours.
    pub files: u64,
}

impl Benchmark {
    /// The share of tokens saved, as a whole percentage.
    pub fn percent(&self) -> u64 {
        (self.files.saturating_sub(self.graph) * 100 / self.files.max(1)).min(99)
    }
}

/// Measures the saving on a sample of the symbols the project leans on, without inventing the
/// questions: for each one, "what is it connected to?" is asked of the graph, and the baseline is
/// reading the file that defines it and the files of what it is directly linked to. None when the
/// project is too small for the graph to be cheaper than its files.
pub fn benchmark(graph: &Graph) -> Option<Benchmark> {
    let mut symbols: Vec<usize> = (0..graph.nodes.len())
        .filter(|&i| graph.nodes[i].kind != "file" && graph.nodes[i].degree > 0)
        .collect();
    symbols.sort_by_key(|&i| (std::cmp::Reverse(graph.nodes[i].degree), i));
    if symbols.is_empty() {
        return None;
    }
    // The most linked ones, then others spread over the rest.
    let head = 5.min(symbols.len());
    let mut sample: Vec<usize> = symbols[..head].to_vec();
    let rest = &symbols[head..];
    let wanted = SAMPLE.saturating_sub(head).min(rest.len());
    if wanted > 0 {
        sample.extend((0..wanted).map(|k| rest[k * rest.len() / wanted]));
    }
    let size_of_file: std::collections::HashMap<&str, u64> = graph
        .nodes
        .iter()
        .filter(|n| n.kind == "file")
        .map(|n| (n.file.as_str(), n.size))
        .collect();
    let mut neighbours: Vec<Vec<usize>> = vec![Vec::new(); graph.nodes.len()];
    for link in graph.links.iter().filter(|l| l.kind != "contains") {
        neighbours[link.source].push(link.target);
        neighbours[link.target].push(link.source);
    }
    let (mut answered, mut read) = (0u64, 0u64);
    for &symbol in &sample {
        let mut files: Vec<&str> = std::iter::once(graph.nodes[symbol].file.as_str())
            .chain(
                neighbours[symbol]
                    .iter()
                    .map(|&n| graph.nodes[n].file.as_str()),
            )
            .collect();
        files.sort_unstable();
        files.dedup();
        read += files
            .iter()
            .filter_map(|f| size_of_file.get(f))
            .sum::<u64>()
            / CHARS_PER_TOKEN;
        let name = &graph.nodes[symbol].label;
        answered += super::query::node(graph, name, ANSWER_BUDGET).len() as u64 / CHARS_PER_TOKEN;
    }
    let questions = sample.len() as u64;
    let benchmark = Benchmark {
        questions: sample.len(),
        graph: answered / questions,
        files: read / questions,
    };
    (benchmark.files > benchmark.graph).then_some(benchmark)
}

/// The report: what the project is made of, what everything leans on, how it is grouped. It is
/// written for an AI to read first, in English.
pub fn markdown(graph: &Graph) -> String {
    let links = graph.links.iter().filter(|l| l.kind != "contains").count();
    let mut body = format!(
        "{} files · {} symbols · {} links · {} groups — generated {} (UTC) by Atelier, no AI involved.\n\n",
        files(graph),
        symbols(graph),
        links,
        graph.communities.len(),
        clock(graph.generated)
    );
    body.push_str("Links marked `EXTRACTED` are read from the code; `INFERRED` is a guess from a name that is unique in the project.\n\n");

    let mut central: Vec<usize> = (0..graph.nodes.len())
        .filter(|&i| graph.nodes[i].degree > 0)
        .collect();
    central.sort_by_key(|&i| (std::cmp::Reverse(graph.nodes[i].degree), i));
    body.push_str(
        "## Central elements\n\nWhat the rest of the project leans on most: read these first.\n\n",
    );
    for &i in central.iter().take(20) {
        body.push_str(&format!(
            "- {} — {} links\n",
            describe(graph, i),
            graph.nodes[i].degree
        ));
    }

    body.push_str("\n## File groups\n\n");
    for community in graph.communities.iter().take(15) {
        let mut members = community.files.clone();
        members.sort_by_key(|&n| (std::cmp::Reverse(graph.nodes[n].degree), n));
        let names: Vec<&str> = members
            .iter()
            .take(8)
            .map(|&n| graph.nodes[n].label.as_str())
            .collect();
        let more = members.len().saturating_sub(8);
        body.push_str(&format!(
            "### {}. {} ({} files)\n\n",
            community.id + 1,
            community.label,
            members.len()
        ));
        body.push_str(&format!(
            "Files: {}{}\n",
            names.join(", "),
            if more > 0 {
                format!(" and {more} more")
            } else {
                String::new()
            }
        ));
        let mut key: Vec<usize> = (0..graph.nodes.len())
            .filter(|&i| graph.nodes[i].community == community.id && graph.nodes[i].kind != "file")
            .collect();
        key.sort_by_key(|&i| (std::cmp::Reverse(graph.nodes[i].degree), i));
        let key: Vec<&str> = key
            .iter()
            .take(8)
            .filter(|&&i| graph.nodes[i].degree > 0)
            .map(|&i| graph.nodes[i].label.as_str())
            .collect();
        if !key.is_empty() {
            body.push_str(&format!("Key elements: {}\n", key.join(", ")));
        }
        body.push('\n');
    }
    if graph.communities.len() > 15 {
        body.push_str(&format!(
            "… and {} smaller groups.\n\n",
            graph.communities.len() - 15
        ));
    }

    // The links between groups are where changes spread.
    let mut between: BTreeMap<(usize, usize), (usize, String)> = BTreeMap::new();
    for link in graph.links.iter().filter(|l| l.kind != "contains") {
        let (a, b) = (&graph.nodes[link.source], &graph.nodes[link.target]);
        if a.community != b.community {
            let key = (a.community.min(b.community), a.community.max(b.community));
            let entry = between
                .entry(key)
                .or_insert((0, format!("{} → {}", a.label, b.label)));
            entry.0 += 1;
        }
    }
    let mut pairs: Vec<_> = between.into_iter().collect();
    pairs.sort_by_key(|(key, (count, _))| (std::cmp::Reverse(*count), *key));
    if !pairs.is_empty() {
        body.push_str(
            "## Links between groups\n\nWhere a change in one group reaches another.\n\n",
        );
        for ((a, b), (count, example)) in pairs.into_iter().take(8) {
            body.push_str(&format!(
                "- {} ↔ {}: {count} links (e.g. {example})\n",
                graph.communities[a].label, graph.communities[b].label
            ));
        }
        body.push('\n');
    }
    body.push_str("---\nTo dig further, use the MCP tools `graph_query`, `graph_node` and `graph_path` of Atelier.\n");

    let mut out = String::from("# Context graph\n\n");
    match benchmark(graph) {
        Some(b) => out.push_str(&format!(
            "**Estimated token saving: up to ~{}% per question.** Measured on {} symbols of this project: asking this graph what a symbol is connected to costs ~{} tokens on average, \
             against ~{} tokens to read the file that defines it and the files of what it is directly linked to ({CHARS_PER_TOKEN} characters per token). \
             This is an upper bound on one question, not on the whole project: once the graph has located something, the AI still opens the part of the code it needs.\n\n",
            b.percent(),
            b.questions,
            tokens(b.graph),
            tokens(b.files)
        )),
        None => out.push_str("The project is small: a question to the graph would cost about as much as reading the files involved, so there is little to save yet.\n\n"),
    }
    out.push_str(&body);
    out
}

/// The graph as node-link JSON: nodes by id, links between ids.
pub fn json(graph: &Graph) -> Value {
    json!({
        "directed": true,
        "generated": graph.generated,
        "nodes": graph.nodes.iter().map(|n| json!({
            "id": n.id, "label": n.label, "kind": n.kind, "file": n.file, "line": n.line, "community": n.community, "degree": n.degree,
        })).collect::<Vec<_>>(),
        "links": graph.links.iter().map(|l| json!({
            "source": graph.nodes[l.source].id, "target": graph.nodes[l.target].id, "kind": l.kind, "confidence": l.confidence,
        })).collect::<Vec<_>>(),
        "communities": graph.communities.iter().map(|c| json!({
            "id": c.id, "label": c.label, "files": c.files.iter().map(|&f| graph.nodes[f].file.clone()).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
#[path = "../../test/graph/report/tests.rs"]
mod tests;
