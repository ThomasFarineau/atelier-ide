//! dryer: candidate duplicate functions by structural fingerprint similarity.

use super::args::{self, Args};
use super::lang::{Lang, parse};
use super::{Out, discover, funcs, metrics, par, progress};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;
use std::time::Instant;
use tree_sitter::Node;

pub struct Form {
    pub file: String,
    pub start: usize,
    pub end: usize,
    rust: bool,
    nodes: usize,
    prints: Vec<u64>,
}

impl Form {
    fn is_big_enough(&self, min_lines: usize, min_nodes: usize) -> bool {
        self.end - self.start + 1 >= min_lines && self.nodes >= min_nodes
    }
}

fn hash<T: Hash>(t: T) -> u64 {
    let mut h = DefaultHasher::new();
    t.hash(&mut h);
    h.finish()
}

/// Named children, comments left out.
fn named_children<'t>(n: Node<'t>) -> Vec<Node<'t>> {
    let mut c = n.walk();
    n.named_children(&mut c)
        .filter(|x| !matches!(x.kind(), "comment" | "line_comment" | "block_comment"))
        .collect()
}

fn is_callee(x: Node) -> bool {
    x.parent().is_some_and(|p| {
        p.kind() == "call_expression" && p.child_by_field_name("function") == Some(x)
    })
}

/// Called names stay in the structure; every other name is incidental.
fn label(n: Node, src: &str) -> String {
    let is_name = matches!(
        n.kind(),
        "identifier"
            | "property_identifier"
            | "type_identifier"
            | "field_identifier"
            | "shorthand_property_identifier"
            | "shorthand_property_identifier_pattern"
            | "statement_identifier"
    );
    let member_callee =
        |p: Node| matches!(p.kind(), "member_expression" | "field_expression") && is_callee(p);
    if !is_name {
        n.kind().to_string()
    } else if is_callee(n) || n.parent().is_some_and(member_callee) {
        format!("call:{}", &src[n.byte_range()])
    } else {
        "id".into()
    }
}

/// Add the fingerprints of `n` and its descendants; returns how many nodes were visited.
fn collect(n: Node, src: &str, parent: &str, prints: &mut Vec<u64>) -> usize {
    let me = label(n, src);
    let kids = named_children(n);
    let kid_labels: Vec<String> = kids.iter().map(|k| label(*k, src)).collect();
    prints.push(hash(format!("{me}({})", kid_labels.join(","))));
    prints.push(hash(format!("{parent}>{me}")));
    1 + kids
        .into_iter()
        .map(|k| collect(k, src, &me, prints))
        .sum::<usize>()
}

fn fingerprint(node: Node, src: &str) -> (Vec<u64>, usize) {
    let mut prints = Vec::new();
    let nodes = collect(node, src, "", &mut prints);
    prints.sort_unstable();
    prints.dedup();
    (prints, nodes)
}

fn forms_in_source(lang: Lang, src: &str, file: &str) -> Vec<Form> {
    let Some(tree) = parse(lang, src) else {
        return vec![];
    };
    funcs::functions(&tree, src, lang, "")
        .iter()
        .map(|f| {
            let (prints, nodes) = fingerprint(f.node, src);
            Form {
                file: file.to_string(),
                start: f.start_line,
                end: f.end_line,
                rust: lang.is_rust(),
                nodes,
                prints,
            }
        })
        .collect()
}

pub fn forms_of(root: &Path, file: &Path) -> Vec<Form> {
    let Some(lang) = Lang::from_path(file) else {
        return vec![];
    };
    let Ok(src) = std::fs::read_to_string(file) else {
        return vec![];
    };
    forms_in_source(lang, &src, &discover::rel(root, file))
}

pub fn similarity(a: &[u64], b: &[u64]) -> f64 {
    let (mut i, mut j, mut shared) = (0, 0, 0usize);
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            // Sorted and unique: after a match the next `b` entry is larger, so `j` advances next turn.
            std::cmp::Ordering::Equal => {
                shared += 1;
                i += 1;
            }
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
        }
    }
    let union = a.len() + b.len() - shared;
    if union == 0 {
        0.0
    } else {
        shared as f64 / union as f64
    }
}

pub struct Candidate<'a> {
    pub score: f64,
    pub a: &'a Form,
    pub b: &'a Form,
}

fn compare_row<'a>(eligible: &[&'a Form], i: usize, threshold: f64) -> Vec<Candidate<'a>> {
    let a = eligible[i];
    eligible[i + 1..]
        .iter()
        .filter(|b| a.rust == b.rust)
        .map(|b| (similarity(&a.prints, &b.prints), *b))
        .filter(|(score, _)| *score >= threshold)
        .map(|(score, b)| Candidate { score, a, b })
        .collect()
}

/// Pairs of forms at least `threshold` similar, best first; rows of pairs compared on `jobs` threads.
pub fn candidates<'a>(
    forms: &'a [Form],
    threshold: f64,
    min_lines: usize,
    min_nodes: usize,
    jobs: usize,
) -> Vec<Candidate<'a>> {
    let eligible: Vec<&Form> = forms
        .iter()
        .filter(|f| f.is_big_enough(min_lines, min_nodes))
        .collect();
    let rows: Vec<usize> = (0..eligible.len()).collect();
    let mut found: Vec<Candidate> =
        par::map(&rows, jobs, |i| compare_row(&eligible, *i, threshold))
            .into_iter()
            .flatten()
            .collect();
    found.sort_by(|x, y| y.score.partial_cmp(&x.score).unwrap());
    found
}

fn edn(cands: &[Candidate]) -> String {
    let form = |f: &Form| format!("{{:file \"{}\" :start {} :end {}}}", f.file, f.start, f.end);
    let rows: Vec<String> = cands
        .iter()
        .map(|c| {
            format!(
                "{{:score {:.2} :forms [{} {}]}}",
                c.score,
                form(c.a),
                form(c.b)
            )
        })
        .collect();
    format!("{{:candidates [{}]}}\n", rows.join("\n             "))
}

struct Options {
    threshold: f64,
    min_lines: usize,
    min_nodes: usize,
    jobs: usize,
}

fn options(args: &Args) -> Result<Options, String> {
    Ok(Options {
        threshold: args.number("threshold", 0.8)?,
        min_lines: args.number("min-lines", 4)?,
        min_nodes: args.number("min-nodes", 20)?,
        jobs: par::jobs(args)?,
    })
}

fn print_candidates(found: &[Candidate], out: &Out) {
    for c in found {
        out.line(&format!("DUPLICATE score={:.2}", c.score));
        for f in [c.a, c.b] {
            out.line(&format!("  {}:{}-{}", f.file, f.start, f.end));
        }
    }
    if found.is_empty() {
        out.line("No duplicate candidates.");
    }
}

/// The options `dryer` takes.
pub const FLAGS: &[&str] = &["changed"];
pub const VALUES: &[&str] = &["threshold", "min-lines", "min-nodes", "jobs"];

/// Exit 0 when there is no duplicate, 2 when candidates are found, 1 on an error.
pub fn run_in(root: &Path, argv: &[String], out: &Out) -> i32 {
    args::exit("dryer", execute(root, argv, out), out)
}

fn execute(root: &Path, argv: &[String], out: &Out) -> Result<i32, String> {
    let args = Args::parse(argv, FLAGS, VALUES)?;
    let opts = options(&args)?;
    let files = discover::select(root, &args.positional, args.has("changed"))?;
    if files.is_empty() {
        out.line("No source files to analyze.");
        return Ok(0);
    }
    let start = Instant::now();
    let forms: Vec<Form> = par::map(&files, opts.jobs, |f| forms_of(root, f))
        .into_iter()
        .flatten()
        .collect();
    let found = candidates(
        &forms,
        opts.threshold,
        opts.min_lines,
        opts.min_nodes,
        opts.jobs,
    );
    out.line(&progress::summary(
        &format!(
            "compared {} functions from {} files",
            forms.len(),
            files.len()
        ),
        opts.jobs,
        start.elapsed(),
    ));
    metrics::write(root, "dry.edn", &edn(&found), out);
    print_candidates(&found, out);
    Ok(if found.is_empty() { 0 } else { 2 })
}

#[cfg(test)]
#[path = "../../../../../test/plugins/builtins/mcp/quality/dryer/tests.rs"]
mod tests;
