//! crapper: CRAP score (complexity x coverage) per function.

use super::args::{self, Args};
use super::lang::{Lang, parse};
use super::lcov::Coverage;
use super::{Out, coverage, discover, funcs, metrics, par, progress};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Instant;

pub struct Entry {
    pub name: String,
    pub namespace: String,
    pub complexity: usize,
    pub coverage: Option<f64>,
}

impl Entry {
    pub fn crap(&self) -> Option<f64> {
        self.coverage.map(|c| crap(self.complexity, c))
    }
}

/// CRAP = CC^2 * (1 - coverage)^3 + CC, coverage in percent.
pub fn crap(cc: usize, coverage_pct: f64) -> f64 {
    let cc = cc as f64;
    cc * cc * (1.0 - coverage_pct / 100.0).powi(3) + cc
}

pub fn analyze_file(root: &Path, file: &Path, cov: Option<&Coverage>) -> Vec<Entry> {
    let Some(lang) = Lang::from_path(file) else {
        return vec![];
    };
    let Ok(src) = std::fs::read_to_string(file) else {
        return vec![];
    };
    let Some(tree) = parse(lang, &src) else {
        return vec![];
    };
    let module = funcs::module_name(root, file, lang);
    funcs::functions(&tree, &src, lang, &module)
        .iter()
        .map(|f| Entry {
            name: f.name.clone(),
            namespace: f.namespace.clone(),
            complexity: funcs::complexity(f.node),
            coverage: cov.map(|c| c.function_percent(file, f.start_line, f.end_line)),
        })
        .collect()
}

pub fn edn_string(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// A number rounded to one decimal, or `nil`.
pub fn edn_number(o: Option<f64>) -> String {
    o.map_or("nil".to_string(), |v| {
        ((v * 10.0).round() / 10.0).to_string()
    })
}

pub fn to_edn(entries: &[Entry]) -> String {
    let rows: Vec<String> = entries
        .iter()
        .map(|e| {
            format!(
                "{{:name {} :namespace {} :complexity {} :coverage {} :crap {}}}",
                edn_string(&e.name),
                edn_string(&e.namespace),
                e.complexity,
                edn_number(e.coverage),
                edn_number(e.crap()),
            )
        })
        .collect();
    format!("{{:entries [{}]}}\n", rows.join("\n           "))
}

fn cell(o: Option<f64>) -> String {
    o.map_or("N/A".to_string(), |v| format!("{v:.1}"))
}

fn table(entries: &[&Entry], languages: &BTreeSet<&str>) -> String {
    let w_name = entries.iter().map(|e| e.name.len()).fold(8, usize::max);
    let w_ns = entries
        .iter()
        .map(|e| e.namespace.len())
        .fold(9, usize::max);
    let mut out = format!(
        "CRAP Report\n===========\nLanguages: {}\n\n{:<w_name$}  {:<w_ns$}  {:>3}  {:>6}  {:>7}\n{}\n",
        languages.iter().copied().collect::<Vec<_>>().join(", "),
        "Function",
        "Namespace",
        "CC",
        "Cov%",
        "CRAP",
        "-".repeat(w_name + w_ns + 24),
    );
    for e in entries {
        out += &format!(
            "{:<w_name$}  {:<w_ns$}  {:>3}  {:>6}  {:>7}\n",
            e.name,
            e.namespace,
            e.complexity,
            cell(e.coverage),
            cell(e.crap())
        );
    }
    out
}

/// Coverage as the `--no-coverage` and `--use-existing-coverage` flags ask; otherwise measured now.
pub fn load_coverage(
    args: &Args,
    root: &Path,
    files: &[PathBuf],
    out: &Out,
) -> Result<Option<Coverage>, String> {
    if args.has("no-coverage") {
        return Ok(None);
    }
    if args.has("use-existing-coverage") {
        return coverage::load_existing(root)
            .map(Some)
            .ok_or("no lcov report on disk".into());
    }
    let (script, rust) = coverage::kinds(files);
    coverage::run(root, script, rust, out).map(Some)
}

fn worst_first(entries: &[Entry]) -> Vec<&Entry> {
    let key = |e: &Entry| (e.crap().unwrap_or(0.0), e.complexity as f64);
    let mut shown: Vec<&Entry> = entries.iter().collect();
    shown.sort_by(|a, b| key(b).partial_cmp(&key(a)).unwrap());
    shown
}

fn exit_code(args: &Args, worst: Option<f64>) -> Result<i32, String> {
    let limit = args.number::<f64>("threshold", 10.0)?;
    Ok(if worst.is_some_and(|w| w > limit) {
        2
    } else {
        0
    })
}

/// The options `crapper` takes.
pub const FLAGS: &[&str] = &["no-coverage", "use-existing-coverage", "changed"];
pub const VALUES: &[&str] = &["threshold", "jobs"];

/// Exit 0, 2 when the worst CRAP is above `--threshold`, 1 on an error.
pub fn run_in(root: &Path, argv: &[String], out: &Out) -> i32 {
    args::exit("crapper", execute(root, argv, out), out)
}

fn execute(root: &Path, argv: &[String], out: &Out) -> Result<i32, String> {
    let args = Args::parse(argv, FLAGS, VALUES)?;
    let files = discover::select(root, &args.positional, args.has("changed"))?;
    if files.is_empty() {
        out.line("No source files to analyze.");
        return Ok(0);
    }
    let cov = load_coverage(&args, root, &files, out)?;
    let (threads, start) = (par::jobs(&args)?, Instant::now());
    let entries: Vec<Entry> = par::map(&files, threads, |f| analyze_file(root, f, cov.as_ref()))
        .into_iter()
        .flatten()
        .collect();
    out.line(&progress::summary(
        &format!(
            "analyzed {} functions in {} files",
            entries.len(),
            files.len()
        ),
        threads,
        start.elapsed(),
    ));
    let languages: BTreeSet<&str> = files
        .iter()
        .filter_map(|f| Lang::from_path(f))
        .map(Lang::name)
        .collect();
    let shown = worst_first(&entries);
    out.print(&table(&shown, &languages));
    metrics::write(root, "crap.edn", &to_edn(&entries), out);
    exit_code(&args, shown.first().and_then(|e| e.crap()))
}

#[cfg(test)]
#[path = "../../../../../test/plugins/builtins/mcp/quality/crapper/tests.rs"]
mod tests;
