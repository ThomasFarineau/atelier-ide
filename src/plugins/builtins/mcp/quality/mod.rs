//! The quality gates of the MCP server: CRAP scores per function ([`crapper`]), duplicate
//! functions ([`dryer`]), functions worth merging into their only caller ([`verifier`]) and
//! mutation testing ([`mutator`]), for TypeScript, JavaScript and Rust. `checker` runs the first
//! three, `cleaner` all four.
//!
//! Each gate is a tool, run on the thread of the connection (it can take minutes: coverage, tests),
//! and a command (an MCP prompt, [`prompts`]) that tells the AI what to do with its findings. The
//! snapshots are written under `.metrics/` in the project.

mod args;
mod coverage;
mod crapper;
mod discover;
mod dryer;
mod funcs;
mod lang;
mod lcov;
mod metrics;
mod mutator;
mod par;
mod progress;
mod prompts;
mod run;
mod shell;
mod verifier;
mod workers;

pub use progress::fmt_duration;
pub use prompts::{prompt, prompts};
pub use run::{Progress, current, paused, set_paused};

use serde_json::{Value, json};
use std::{path::Path, sync::Mutex};

/// The report of a run, written line by line (by several threads at once for the mutator).
#[derive(Default)]
pub struct Out(Mutex<String>);

impl Out {
    pub fn print(&self, text: &str) {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push_str(text);
    }

    pub fn line(&self, text: &str) {
        self.print(&format!("{text}\n"));
    }

    pub fn take(self) -> String {
        self.0.into_inner().unwrap_or_else(|e| e.into_inner())
    }
}

/// A gate: its tool, how it runs, and what its exit codes mean.
struct Gate {
    name: &'static str,
    run: fn(&Path, &[String], &Out) -> i32,
    verdicts: &'static [(i32, &'static str)],
}

const CRAPPER: Gate = Gate {
    name: "crapper",
    run: crapper::run_in,
    verdicts: &[
        (0, "pass: no function above the CRAP threshold"),
        (2, "fail: functions above the CRAP threshold"),
    ],
};
const DRYER: Gate = Gate {
    name: "dryer",
    run: dryer::run_in,
    verdicts: &[(0, "pass: no duplicate"), (2, "fail: duplicate candidates")],
};
const VERIFIER: Gate = Gate {
    name: "verifier",
    run: verifier::run_in,
    verdicts: &[(0, "pass: nothing to merge"), (2, "fail: merges proposed")],
};
const MUTATOR: Gate = Gate {
    name: "mutator",
    run: mutator::run_in,
    verdicts: &[
        (0, "pass: every mutant killed"),
        (2, "fail: the tests fail on the unmutated code"),
        (3, "fail: mutants survived"),
    ],
};

impl Gate {
    fn verdict(&self, code: i32) -> &'static str {
        self.verdicts
            .iter()
            .find(|(c, _)| *c == code)
            .map_or("error", |(_, v)| v)
    }
}

/// The tools: a gate alone, or several in a row.
const TOOLS: &[(&str, &[Gate], &str)] = &[
    (
        "crapper",
        &[CRAPPER],
        "CRAP score of every function (cyclomatic complexity × lack of coverage: CC² × (1 − cov)³ + CC), worst first, for TypeScript, JavaScript and Rust. Measures the coverage first (npm/vitest/c8, cargo llvm-cov or tarpaulin) unless told otherwise. Fails when a function is above the threshold (10). Writes .metrics/crap.edn.",
    ),
    (
        "dryer",
        &[DRYER],
        "Candidate duplicate functions, by similarity of their syntax structure (local names ignored, called names kept), best first. Fails when a pair reaches the threshold (0.8). Writes .metrics/dry.edn.",
    ),
    (
        "verifier",
        &[VERIFIER],
        "Functions worth merging into their only caller, after a split: defined once, mentioned once (tests included) by a call that is not in a closure, no early exit, and whose CRAP plus the caller's is under the threshold (8). Fails when merges are proposed. Writes .metrics/merge.edn.",
    ),
    (
        "mutator",
        &[MUTATOR],
        "Mutation testing: flips operators and literals one at a time in copies of the project (target/mutation-workers) and runs the tests; a mutant must make them fail. Uncovered lines are counted, not run; unchanged functions whose mutants were all killed are not run again. Long: the tests run once per mutant. Fails when a mutant survives. Writes .metrics/mutate/.",
    ),
    (
        "checker",
        &[CRAPPER, DRYER, VERIFIER],
        "crapper, dryer then verifier, with one coverage measurement: the report of each and a summary.",
    ),
    (
        "cleaner",
        &[CRAPPER, DRYER, VERIFIER, MUTATOR],
        "crapper, dryer, verifier then mutator, with one coverage measurement: the report of each and a summary. Long: the mutator runs the tests once per mutant.",
    ),
];

/// An MCP argument of the gates: its name, its JSON type, the option it becomes, the gates that
/// take it, and its description.
struct Arg {
    key: &'static str,
    kind: &'static str,
    option: &'static str,
    gates: &'static [&'static str],
    description: &'static str,
}

const COVERED: &[&str] = &["crapper", "verifier", "mutator"];
const ALL: &[&str] = &["crapper", "dryer", "verifier", "mutator"];

const OPTIONS: &[Arg] = &[
    Arg {
        key: "changed",
        kind: "boolean",
        option: "changed",
        gates: ALL,
        description: "Only the files git sees added or changed.",
    },
    Arg {
        key: "noCoverage",
        kind: "boolean",
        option: "no-coverage",
        gates: COVERED,
        description: "Do not measure the coverage: complexity alone (the lowest CRAP possible), and the mutator runs every site.",
    },
    Arg {
        key: "useExistingCoverage",
        kind: "boolean",
        option: "use-existing-coverage",
        gates: COVERED,
        description: "Read the lcov reports already on disk instead of running the tests.",
    },
    Arg {
        key: "jobs",
        kind: "integer",
        option: "jobs",
        gates: ALL,
        description: "Threads (workers for the mutator); one per core by default.",
    },
    Arg {
        key: "crapThreshold",
        kind: "number",
        option: "threshold",
        gates: &["crapper"],
        description: "crapper fails when a function's CRAP is above it; 10 by default.",
    },
    Arg {
        key: "dryThreshold",
        kind: "number",
        option: "threshold",
        gates: &["dryer"],
        description: "Minimum similarity of a duplicate, from 0 to 1; 0.8 by default.",
    },
    Arg {
        key: "minLines",
        kind: "integer",
        option: "min-lines",
        gates: &["dryer"],
        description: "Functions shorter than this many lines are not compared; 4 by default.",
    },
    Arg {
        key: "minNodes",
        kind: "integer",
        option: "min-nodes",
        gates: &["dryer"],
        description: "Functions with fewer syntax nodes are not compared; 20 by default.",
    },
    Arg {
        key: "mergeThreshold",
        kind: "number",
        option: "threshold",
        gates: &["verifier"],
        description: "A merge is proposed when the CRAP of the function plus its caller's is under it; 8 by default.",
    },
    Arg {
        key: "scan",
        kind: "boolean",
        option: "scan",
        gates: &["mutator"],
        description: "List the mutation sites, run no test.",
    },
    Arg {
        key: "mutateAll",
        kind: "boolean",
        option: "mutate-all",
        gates: &["mutator"],
        description: "Run again the functions whose mutants were all killed last time.",
    },
    Arg {
        key: "testCommand",
        kind: "string",
        option: "test-command",
        gates: &["mutator"],
        description: "The command that runs the tests (by default cargo test, or the test script of package.json).",
    },
    Arg {
        key: "focusCommand",
        kind: "string",
        option: "focus-command",
        gates: &["mutator"],
        description: "The tests of one module, tried before the full suite; {module} and {file} are filled in (Rust default: cargo test -- {module}::).",
    },
    Arg {
        key: "cleanWorkers",
        kind: "boolean",
        option: "clean-workers",
        gates: &["mutator"],
        description: "Delete the copies of the project when done (they are kept to keep the builds warm).",
    },
];

/// The tools, as `tools/list` describes them.
pub fn specs() -> Vec<Value> {
    TOOLS
        .iter()
        .map(|(name, gates, description)| {
            let mut properties = serde_json::Map::new();
            properties.insert("paths".into(), json!({
                "type": "array",
                "items": {"type": "string"},
                "description": "Files, folders or fragments of paths to look at, from the project; the whole project by default (test files and folders, node_modules, target… are left out).",
            }));
            for o in OPTIONS
                .iter()
                .filter(|o| gates.iter().any(|g| o.gates.contains(&g.name)))
            {
                properties.insert(
                    o.key.into(),
                    json!({"type": o.kind, "description": o.description}),
                );
            }
            json!({
                "name": name,
                "description": description,
                "inputSchema": {"type": "object", "properties": properties},
            })
        })
        .collect()
}

/// The paths of the arguments: a list, or one text with the paths separated by spaces.
fn paths(args: &Value) -> Result<Vec<String>, String> {
    match &args["paths"] {
        Value::Null => Ok(Vec::new()),
        Value::String(s) => Ok(s.split_whitespace().map(String::from).collect()),
        Value::Array(items) => items
            .iter()
            .map(|p| {
                p.as_str()
                    .map(String::from)
                    .ok_or_else(|| "“paths” holds texts".to_string())
            })
            .collect(),
        _ => Err("“paths” is a list of paths".into()),
    }
}

/// The command-line words of `gate` for the MCP arguments: the paths, then the options it takes.
fn argv(args: &Value, gate: &str) -> Result<Vec<String>, String> {
    let mut words = paths(args)?;
    for (key, value) in args.as_object().into_iter().flatten() {
        if key == "paths" {
            continue;
        }
        let Some(o) = OPTIONS.iter().find(|o| o.key == key) else {
            return Err(format!("unknown argument “{key}”"));
        };
        if !o.gates.contains(&gate) {
            continue;
        }
        match value {
            Value::Bool(true) => words.push(format!("--{}", o.option)),
            Value::Bool(false) | Value::Null => {}
            Value::Number(n) => words.push(format!("--{}={n}", o.option)),
            Value::String(s) => words.push(format!("--{}={s}", o.option)),
            _ => return Err(format!("“{key}” is a {}", o.kind)),
        }
    }
    Ok(words)
}

/// Runs `gates` in a row. The first gate that measures the coverage leaves its reports on disk:
/// the next ones read them instead of running the tests again.
fn run(tool: &str, gates: &[Gate], args: &Value, root: &Path) -> Result<String, String> {
    let mut words = gates
        .iter()
        .map(|g| argv(args, g.name))
        .collect::<Result<Vec<_>, _>>()?;
    let _run = run::start(tool)?;
    let mut measured = false;
    let mut sections = Vec::new();
    for (gate, words) in gates.iter().zip(&mut words) {
        let covered = COVERED.contains(&gate.name);
        let chosen = words
            .iter()
            .any(|w| w == "--no-coverage" || w == "--use-existing-coverage");
        if covered && measured && !chosen {
            words.push("--use-existing-coverage".into());
        }
        run::checkpoint();
        run::gate(gate.name);
        let out = Out::default();
        let code = (gate.run)(root, words, &out);
        measured |= covered && !chosen && code != 1 && !words.iter().any(|w| w == "--scan");
        sections.push((gate, code, out.take()));
    }
    if let [(gate, code, text)] = sections.as_slice() {
        return Ok(format!("{text}\nResult: {}", gate.verdict(*code)));
    }
    let mut report = String::new();
    for (gate, code, text) in &sections {
        report += &format!("## {}: {}\n\n{text}\n", gate.name, gate.verdict(*code));
    }
    report += "## Summary\n\n";
    for (gate, code, _) in &sections {
        report += &format!("- {}: {}\n", gate.name, gate.verdict(*code));
    }
    Ok(report)
}

/// Runs a quality tool. None for another tool.
pub fn tool(name: &str, args: &Value, root: &Path) -> Option<Result<String, String>> {
    let (_, gates, _) = TOOLS.iter().find(|(n, _, _)| *n == name)?;
    Some(run(name, gates, args, root))
}

/// A folder of the temporary directory, deleted with it: a project for the tests.
#[cfg(test)]
pub struct Scratch(std::path::PathBuf);

#[cfg(test)]
impl Scratch {
    pub fn path(&self) -> &Path {
        &self.0
    }
}

#[cfg(test)]
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
pub fn scratch() -> Scratch {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "atelier-quality-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temporary folder");
    Scratch(dir)
}

#[cfg(test)]
#[path = "../../../../../test/plugins/builtins/mcp/quality/mod/tests.rs"]
mod tests;
