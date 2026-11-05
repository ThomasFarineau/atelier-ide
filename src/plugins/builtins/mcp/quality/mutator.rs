//! mutator: mutation testing. A mutant is killed when the tests fail or time out.

use super::args::{self, Args};
use super::lang::{Lang, parse};
use super::lcov::Coverage;
use super::progress;
use super::shell::{self, Outcome};
use super::{Out, coverage, discover, funcs, par, run, workers};
use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tree_sitter::Node;

#[derive(Clone)]
pub struct Site {
    pub line: usize,
    pub start: usize,
    pub end: usize,
    pub from: String,
    pub to: String,
}

const SWAPS: &[(&str, &str)] = &[
    (">", ">="),
    (">=", ">"),
    ("<", "<="),
    ("<=", "<"),
    ("==", "!="),
    ("!=", "=="),
    ("===", "!=="),
    ("!==", "==="),
    ("&&", "||"),
    ("||", "&&"),
    ("+", "-"),
    ("-", "+"),
    ("*", "/"),
    ("/", "*"),
];

fn flip_literal(kind: &str, text: &str) -> Option<String> {
    match kind {
        "true" => Some("false".into()),
        "false" => Some("true".into()),
        "boolean_literal" => Some(if text == "true" { "false" } else { "true" }.into()),
        "number" | "integer_literal" => {
            let to = if text == "0" { "1" } else { "0" };
            (to != text).then(|| to.to_string())
        }
        _ => None,
    }
}

const TYPE_NODES: &[&str] = &[
    "type_annotation",
    "type_alias_declaration",
    "interface_declaration",
    "literal_type",
    "type_arguments",
];

fn site(n: Node, line: usize, from: &str, to: &str) -> Site {
    Site {
        line,
        start: n.start_byte(),
        end: n.end_byte(),
        from: from.into(),
        to: to.into(),
    }
}

pub fn sites_in(node: Node, src: &str) -> Vec<Site> {
    let mut out = Vec::new();
    collect(node, src, &mut out);
    out
}

fn collect(n: Node, src: &str, out: &mut Vec<Site>) {
    if TYPE_NODES.contains(&n.kind()) {
        return;
    }
    let line = n.start_position().row + 1;
    let text = &src[n.byte_range()];
    if n.kind() == "binary_expression"
        && let Some(op) = n.child_by_field_name("operator")
        && let Some((_, to)) = SWAPS
            .iter()
            .find(|(from, _)| *from == &src[op.byte_range()])
    {
        out.push(site(
            op,
            op.start_position().row + 1,
            &src[op.byte_range()],
            to,
        ));
    } else if n.is_named()
        && let Some(to) = flip_literal(n.kind(), text)
    {
        out.push(site(n, line, text, &to));
    }
    let mut cursor = n.walk();
    for child in n.children(&mut cursor) {
        collect(child, src, out);
    }
}

struct FnJob {
    key: String,
    name: String,
    hash: String,
    sites: Vec<Site>,
}

struct FileJob {
    path: PathBuf,
    rel: String,
    module: String,
    lang: Lang,
    src: String,
    fns: Vec<FnJob>,
}

fn hash_text(s: &str) -> String {
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    format!("{:016x}", h.finish())
}

fn load_job(root: &Path, path: &Path) -> Option<FileJob> {
    let lang = Lang::from_path(path)?;
    let src = std::fs::read_to_string(path).ok()?;
    let tree = parse(lang, &src)?;
    let module = funcs::module_name(root, path, lang);
    let mut seen: HashMap<String, usize> = HashMap::new();
    let fns = funcs::functions(&tree, &src, lang, &module)
        .iter()
        .map(|f| {
            let name = f.name.replace('"', "'");
            let n = seen.entry(name.clone()).or_insert(0);
            let key = format!("{name}#{n}");
            *n += 1;
            FnJob {
                key,
                name,
                hash: hash_text(&src[f.node.byte_range()]),
                sites: sites_in(f.node, &src),
            }
        })
        .collect();
    Some(FileJob {
        path: path.to_path_buf(),
        rel: discover::rel(root, path),
        module,
        lang,
        src,
        fns,
    })
}

#[derive(Default, Clone, Debug, PartialEq)]
struct Counts {
    killed: usize,
    survived: usize,
    uncovered: usize,
}

impl Counts {
    fn add(&mut self, other: &Counts) {
        self.killed += other.killed;
        self.survived += other.survived;
        self.uncovered += other.uncovered;
    }

    fn score(&self) -> f64 {
        let ran = self.killed + self.survived;
        if ran == 0 {
            100.0
        } else {
            self.killed as f64 * 100.0 / ran as f64
        }
    }
}

#[derive(Clone)]
struct FnResult {
    line: String,
    counts: Counts,
}

fn str_field<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    let rest = &line[line.find(&format!("{name} \""))? + name.len() + 2..];
    Some(&rest[..rest.find('"')?])
}

fn num_field(line: &str, name: &str) -> usize {
    line.find(&format!("{name} "))
        .map(|i| {
            line[i + name.len() + 1..]
                .chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>()
        })
        .and_then(|d| d.parse().ok())
        .unwrap_or(0)
}

fn snapshot_path(root: &Path, module: &str) -> PathBuf {
    root.join(".metrics/mutate")
        .join(format!("{}.edn", module.replace("::", ".")))
}

/// A function's last run: its text hash, counts and the snapshot line itself.
struct Previous {
    hash: String,
    counts: Counts,
    line: String,
}

fn parse_previous(line: &str) -> Option<(String, Previous)> {
    let counts = Counts {
        killed: num_field(line, ":killed"),
        survived: num_field(line, ":survived"),
        uncovered: num_field(line, ":uncovered"),
    };
    let previous = Previous {
        hash: str_field(line, ":hash")?.to_string(),
        counts,
        line: line.to_string(),
    };
    Some((str_field(line, ":key")?.to_string(), previous))
}

fn read_previous(path: &Path) -> HashMap<String, Previous> {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    text.lines()
        .filter(|l| l.trim_start().starts_with("{:key"))
        .filter_map(parse_previous)
        .collect()
}

fn write_snapshot(root: &Path, job: &FileJob, results: &[FnResult]) {
    let path = snapshot_path(root, &job.module);
    let lines: Vec<&str> = results.iter().map(|r| r.line.as_str()).collect();
    let text = format!(
        "{{:file \"{}\"\n :namespace \"{}\"\n :functions [\n{}\n ]}}\n",
        job.rel,
        job.module,
        lines.join("\n")
    );
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, text);
}

/// How one language family is tested. A mutant goes through the focused tests of its module,
/// then the full suite; the first that does not pass kills it.
#[derive(Debug, PartialEq)]
struct Suite {
    /// Tests of one module; `{module}` and `{file}` are filled in.
    focus: Option<String>,
    full: String,
}

/// Baselines and checks run unmutated code, which should not loop; they still get a bound.
const SLOW: Duration = Duration::from_secs(900);

fn rust_suite(root: &Path, focus: Option<String>) -> Suite {
    let base = if root.join("src/lib.rs").is_file() {
        "cargo test --lib"
    } else {
        "cargo test"
    };
    Suite {
        focus: focus.or_else(|| Some(format!("{base} --quiet -- {{module}}::"))),
        full: format!("{base} --quiet"),
    }
}

fn suite(args: &Args, root: &Path, lang: Lang) -> Result<Suite, String> {
    let focus = args.value("focus-command").map(String::from);
    match args.value("test-command") {
        Some(c) => Ok(Suite {
            focus,
            full: c.to_string(),
        }),
        None if lang.is_rust() => Ok(rust_suite(root, focus)),
        None => Ok(Suite {
            focus,
            full: coverage::node_test_command(root)?,
        }),
    }
}

/// What focused tests filter on: a Rust module path inside its crate, or a dotted script module.
fn focus_module(module: &str, lang: Lang) -> &str {
    if lang.is_rust() {
        module.split_once("::").map_or("", |(_, inner)| inner)
    } else {
        module
    }
}

/// The focused command for a module, if the suite has one and the module has a name.
fn focus_command(suite: &Suite, module: &str, file: &str) -> Option<String> {
    let template = suite.focus.as_ref().filter(|_| !module.is_empty())?;
    Some(template.replace("{module}", module).replace("{file}", file))
}

/// The commands a mutant goes through, cheapest first.
fn steps(suite: &Suite, module: &str, file: &str) -> Vec<String> {
    focus_command(suite, module, file)
        .into_iter()
        .chain([suite.full.clone()])
        .collect()
}

/// Run the steps in order and stop at the first one that does not pass.
fn first_failure(steps: &[String], run: impl Fn(&str) -> Outcome) -> Outcome {
    steps
        .iter()
        .map(|cmd| run(cmd))
        .find(|o| *o != Outcome::Pass)
        .unwrap_or(Outcome::Pass)
}

/// Run a command in a worker copy, with its own build directory.
fn run_in_worker(cmd: &str, dir: &Path, timeout: Duration) -> Outcome {
    let target = dir.join("target").to_string_lossy().to_string();
    shell::run_quiet(cmd, dir, timeout, &[("CARGO_TARGET_DIR", &target)])
}

fn is_uncovered(cov: Option<&Coverage>, path: &Path, line: usize) -> bool {
    cov.is_some_and(|c| c.file(path).is_none_or(|f| f.lines.get(&line) == Some(&0)))
}

/// A function whose text is unchanged and whose mutants were all killed needs no rerun.
fn is_clean(p: &Previous, f: &FnJob) -> bool {
    p.hash == f.hash && p.counts.survived == 0 && p.counts.uncovered == 0
}

/// How each language family is tested (true = Rust).
type Suites = HashMap<bool, Suite>;
/// Mutant timeout per language family, from the slowest baseline of its full suite.
type Timeouts = HashMap<bool, Duration>;

/// One mutant to try: indexes into the jobs, their functions and their sites.
struct Task {
    file: usize,
    func: usize,
    site: usize,
}

/// What a function needs: nothing (its last result still holds) or its covered sites run.
struct FnPlan {
    reuse: Option<FnResult>,
    skipped: Vec<bool>,
}

fn plan_function(
    prev: Option<&Previous>,
    f: &FnJob,
    job: &FileJob,
    cov: Option<&Coverage>,
    mutate_all: bool,
) -> FnPlan {
    match prev.filter(|p| !mutate_all && is_clean(p, f)) {
        Some(p) => FnPlan {
            reuse: Some(FnResult {
                line: p.line.clone(),
                counts: p.counts.clone(),
            }),
            skipped: vec![],
        },
        None => FnPlan {
            reuse: None,
            skipped: f
                .sites
                .iter()
                .map(|s| is_uncovered(cov, &job.path, s.line))
                .collect(),
        },
    }
}

fn plan(
    jobs: &[FileJob],
    root: &Path,
    cov: Option<&Coverage>,
    mutate_all: bool,
) -> (Vec<Vec<FnPlan>>, Vec<Task>) {
    let mut tasks = Vec::new();
    let plans = jobs
        .iter()
        .enumerate()
        .map(|(file, job)| {
            let previous = read_previous(&snapshot_path(root, &job.module));
            let mut plans = Vec::new();
            for (func, f) in job.fns.iter().enumerate() {
                let p = plan_function(previous.get(&f.key), f, job, cov, mutate_all);
                let runnable = p.skipped.iter().enumerate().filter(|(_, skip)| !**skip);
                tasks.extend(
                    runnable
                        .filter(|_| p.reuse.is_none())
                        .map(|(site, _)| Task { file, func, site }),
                );
                plans.push(p);
            }
            plans
        })
        .collect();
    (plans, tasks)
}

fn describe(job: &FileJob, f: &FnJob, s: &Site) -> String {
    format!("{}:{} {} -> {} [{}]", job.rel, s.line, s.from, s.to, f.name)
}

struct Pool<'a> {
    root: &'a Path,
    jobs: &'a [FileJob],
    suites: &'a Suites,
    timeouts: &'a Timeouts,
    out: &'a Out,
}

impl Pool<'_> {
    fn site(&self, t: &Task) -> (&FileJob, &FnJob, &Site) {
        let job = &self.jobs[t.file];
        (job, &job.fns[t.func], &job.fns[t.func].sites[t.site])
    }

    /// Mutate the file in worker `w`'s copy, run its tests, put the file back.
    fn run_task(&self, w: usize, t: &Task) -> Outcome {
        let (job, _, site) = self.site(t);
        let dir = workers::worker_dir(self.root, w);
        let path = dir.join(&job.rel);
        let mutated = format!(
            "{}{}{}",
            &job.src[..site.start],
            site.to,
            &job.src[site.end..]
        );
        let _ = std::fs::write(&path, mutated);
        let module = focus_module(&job.module, job.lang);
        let family = job.lang.is_rust();
        let steps = steps(&self.suites[&family], module, &job.rel);
        let outcome = first_failure(&steps, |cmd| {
            run_in_worker(cmd, &dir, self.timeouts[&family])
        });
        let _ = std::fs::write(&path, &job.src);
        outcome
    }
}

/// Run every task on `workers` threads, each in its own copy; outcomes keep the task order.
fn run_tasks(pool: &Pool, tasks: &[Task], workers: usize) -> Vec<Outcome> {
    let next = AtomicUsize::new(0);
    let slots: Vec<Mutex<Option<Outcome>>> = tasks.iter().map(|_| Mutex::new(None)).collect();
    let ids: Vec<usize> = (0..workers).collect();
    par::map(&ids, workers, |w| {
        loop {
            run::checkpoint();
            let i = next.fetch_add(1, Ordering::SeqCst);
            let Some(t) = tasks.get(i) else { break };
            let outcome = pool.run_task(*w, t);
            if outcome == Outcome::Pass {
                let (job, f, site) = pool.site(t);
                pool.out
                    .line(&format!("SURVIVED {}", describe(job, f, site)));
            }
            *slots[i].lock().unwrap_or_else(|e| e.into_inner()) = Some(outcome);
            run::tick();
        }
    });
    slots
        .into_iter()
        .map(|m| {
            m.into_inner()
                .unwrap_or_else(|e| e.into_inner())
                .unwrap_or(Outcome::Fail)
        })
        .collect()
}

fn tally(
    job: &FileJob,
    f: &FnJob,
    skipped: &[bool],
    outcomes: &mut std::slice::Iter<Outcome>,
) -> FnResult {
    let (mut counts, mut survivors) = (Counts::default(), Vec::new());
    for (site, skip) in f.sites.iter().zip(skipped) {
        if *skip {
            counts.uncovered += 1;
        } else if outcomes.next() == Some(&Outcome::Pass) {
            counts.survived += 1;
            survivors.push(describe(job, f, site));
        } else {
            counts.killed += 1;
        }
    }
    let list: Vec<String> = survivors
        .iter()
        .map(|s| format!("\"{}\"", s.replace('"', "'")))
        .collect();
    let line = format!(
        "  {{:key \"{}\" :name \"{}\" :hash \"{}\" :killed {} :survived {} :uncovered {} :survivors [{}]}}",
        f.key,
        f.name,
        f.hash,
        counts.killed,
        counts.survived,
        counts.uncovered,
        list.join(" ")
    );
    FnResult { line, counts }
}

/// Turn the flat list of outcomes back into per-file, per-function results.
fn assemble(jobs: &[FileJob], plans: &[Vec<FnPlan>], outcomes: &[Outcome]) -> Vec<Vec<FnResult>> {
    let mut outcomes = outcomes.iter();
    jobs.iter()
        .zip(plans)
        .map(|(job, fn_plans)| {
            let mut results = Vec::new();
            for (f, p) in job.fns.iter().zip(fn_plans) {
                results.push(
                    p.reuse
                        .clone()
                        .unwrap_or_else(|| tally(job, f, &p.skipped, &mut outcomes)),
                );
            }
            results
        })
        .collect()
}

fn scan(jobs: &[FileJob], out: &Out) {
    for job in jobs {
        let total: usize = job.fns.iter().map(|f| f.sites.len()).sum();
        out.line(&format!("Scan: {total} mutation sites in {}", job.rel));
        for f in &job.fns {
            for s in &f.sites {
                out.line(&format!(
                    "  {}:{} {} -> {}  [{}]",
                    job.rel, s.line, s.from, s.to, f.name
                ));
            }
        }
    }
}

/// A mutant gets three baselines plus five seconds, and never less than ten seconds.
fn mutant_timeout(baseline: Duration) -> Duration {
    (baseline * 3 + Duration::from_secs(5)).max(Duration::from_secs(10))
}

/// Time the full tests of every language family in `dir`; failing tests are an error.
fn time_tests(dir: &Path, suites: &Suites) -> Result<HashMap<bool, Duration>, String> {
    suites
        .iter()
        .map(|(family, s)| {
            let start = Instant::now();
            match run_in_worker(&s.full, dir, SLOW) {
                Outcome::Pass => Ok((*family, start.elapsed())),
                _ => Err(format!("baseline tests fail: {}", s.full)),
            }
        })
        .collect()
}

/// What the warm-up hands each worker: the suites and the focused commands to check.
struct Bench<'a> {
    suites: &'a Suites,
    checks: &'a [String],
    workers: usize,
    out: &'a Out,
}

/// Copy the project into worker `w`, time the tests there, and check its share of the focused
/// commands: one failing on unmutated code would kill every mutant of its module.
/// A failure is retried once: with many workers building at once, a slow machine can fail a healthy suite.
fn warm_up(root: &Path, w: usize, bench: &Bench) -> Result<HashMap<bool, Duration>, String> {
    let dir = workers::sync(root, w).map_err(|e| format!("worker {w}: {e}"))?;
    let time = || time_tests(&dir, bench.suites);
    let timings = time().or_else(|_| time())?;
    let mut mine = bench.checks.iter().skip(w).step_by(bench.workers);
    if let Some(c) = mine.find(|c| run_in_worker(c, &dir, SLOW) != Outcome::Pass) {
        return Err(format!("focused tests fail on unmutated code: {c}"));
    }
    bench.out.line(&format!(
        "worker {w} ready (tests ran in {})",
        progress::fmt_duration(timings.values().copied().max().unwrap_or_default())
    ));
    Ok(timings)
}

fn suites(args: &Args, root: &Path, jobs: &[FileJob], tasks: &[Task]) -> Result<Suites, String> {
    let mut suites = Suites::new();
    for t in tasks {
        let lang = jobs[t.file].lang;
        if let std::collections::hash_map::Entry::Vacant(e) = suites.entry(lang.is_rust()) {
            e.insert(suite(args, root, lang)?);
        }
    }
    Ok(suites)
}

/// Every distinct focused command of the files to mutate.
fn focus_checks(suites: &Suites, jobs: &[FileJob], tasks: &[Task]) -> Vec<String> {
    let mut checks = Vec::new();
    for job in tasks.iter().map(|t| &jobs[t.file]) {
        let cmd = focus_command(
            &suites[&job.lang.is_rust()],
            focus_module(&job.module, job.lang),
            &job.rel,
        );
        checks.extend(cmd.filter(|c| !checks.contains(c)));
    }
    checks
}

fn run_all(
    args: &Args,
    root: &Path,
    jobs: &[FileJob],
    tasks: &[Task],
    threads: usize,
    out: &Out,
) -> Result<Vec<Outcome>, String> {
    if tasks.is_empty() {
        return Ok(vec![]);
    }
    let workers = threads.min(tasks.len());
    let suites = suites(args, root, jobs, tasks)?;
    let checks = focus_checks(&suites, jobs, tasks);
    let bench = Bench {
        suites: &suites,
        checks: &checks,
        workers,
        out,
    };
    run::phase(&format!(
        "copying the project and running the tests in {workers} worker(s)"
    ));
    out.line(&format!(
        "{} mutants on {workers} worker(s): copying the project and running the tests once in each",
        tasks.len()
    ));
    let ids: Vec<usize> = (0..workers).collect();
    let mut slowest: HashMap<bool, Duration> = HashMap::new();
    for timings in par::map(&ids, workers, |w| warm_up(root, *w, &bench)) {
        for (family, d) in timings? {
            slowest
                .entry(family)
                .and_modify(|m| *m = (*m).max(d))
                .or_insert(d);
        }
    }
    let timeouts: Timeouts = slowest
        .into_iter()
        .map(|(f, d)| (f, mutant_timeout(d)))
        .collect();
    let pool = Pool {
        root,
        jobs,
        suites: &suites,
        timeouts: &timeouts,
        out,
    };
    run::phase("mutants");
    run::total(tasks.len());
    Ok(run_tasks(&pool, tasks, workers))
}

fn load_coverage(args: &Args, root: &Path, jobs: &[FileJob], out: &Out) -> Option<Coverage> {
    if args.has("no-coverage") {
        return None;
    }
    if args.has("use-existing-coverage") {
        return coverage::load_existing(root);
    }
    let paths: Vec<PathBuf> = jobs.iter().map(|j| j.path.clone()).collect();
    let (script, rust) = coverage::kinds(&paths);
    coverage::run(root, script, rust, out)
        .map_err(|e| out.line(&format!("mutator: no coverage ({e}); running every site")))
        .ok()
}

fn load_jobs(root: &Path, args: &Args) -> Result<Vec<FileJob>, String> {
    let files = discover::select(root, &args.positional, args.has("changed"))?;
    Ok(files.iter().filter_map(|f| load_job(root, f)).collect())
}

/// The options `mutator` takes.
pub const FLAGS: &[&str] = &[
    "scan",
    "changed",
    "mutate-all",
    "no-coverage",
    "use-existing-coverage",
    "clean-workers",
];
pub const VALUES: &[&str] = &["test-command", "focus-command", "jobs"];

/// Exit 0 when every mutant is killed, 2 when the tests fail on the original code, 3 when a
/// mutant survived, 1 on an error.
pub fn run_in(root: &Path, argv: &[String], out: &Out) -> i32 {
    args::exit("mutator", execute(root, argv, out), out)
}

fn execute(root: &Path, argv: &[String], out: &Out) -> Result<i32, String> {
    let args = Args::parse(argv, FLAGS, VALUES)?;
    let threads = par::jobs(&args)?;
    let jobs = load_jobs(root, &args)?;
    if jobs.is_empty() {
        out.line("No source files to analyze.");
        return Ok(0);
    }
    if args.has("scan") {
        scan(&jobs, out);
        return Ok(0);
    }
    Ok(mutate(root, &args, &jobs, threads, out))
}

fn summarize(root: &Path, jobs: &[FileJob], results: &[Vec<FnResult>], out: &Out) -> i32 {
    let mut total = Counts::default();
    for (job, fn_results) in jobs.iter().zip(results) {
        if job.fns.iter().any(|f| !f.sites.is_empty()) {
            write_snapshot(root, job, fn_results);
        }
        fn_results.iter().for_each(|r| total.add(&r.counts));
    }
    out.line(&format!(
        "Mutants: {} killed, {} survived, {} uncovered. Score: {:.1}%",
        total.killed,
        total.survived,
        total.uncovered,
        total.score()
    ));
    if total.survived > 0 { 3 } else { 0 }
}

fn mutate(root: &Path, args: &Args, jobs: &[FileJob], threads: usize, out: &Out) -> i32 {
    let cov = load_coverage(args, root, jobs, out);
    let (plans, tasks) = plan(jobs, root, cov.as_ref(), args.has("mutate-all"));
    let outcomes = match run_all(args, root, jobs, &tasks, threads, out) {
        Ok(o) => o,
        Err(e) => {
            out.line(&format!("mutator: {e}"));
            return 2;
        }
    };
    if args.has("clean-workers") {
        workers::clean(root);
    }
    summarize(root, jobs, &assemble(jobs, &plans, &outcomes), out)
}

#[cfg(test)]
#[path = "../../../../../test/plugins/builtins/mcp/quality/mutator/tests.rs"]
mod tests;
