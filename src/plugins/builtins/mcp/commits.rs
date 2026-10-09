//! Commits through MCP. The AI asks Atelier what the repository's commit conventions are and how
//! the changes could be split, writes the messages, and Atelier makes the commits:
//!
//! - `commit` all: one commit with everything;
//! - `commit` by context: one commit per coherent change (files that lean on each other, the
//!   docs, the dependencies, the CI each on their own), using the context graph when it is on.
//!
//! The messages follow what the repository already does (Conventional Commits, gitmoji or free
//! text; French or English; the types and scopes in use), which is read from its history, and are
//! refused when they do not. No commit ever carries a `Co-authored-by` line or an attribution to an
//! AI: they are removed from the message, whoever wrote them. Nothing is pushed, nothing amended.

use crate::plugins::builtins::git::repo::{Change, Console, Kind, Repo, State};
use regex::Regex;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::Path,
    sync::OnceLock,
};

/// How many commits of the history say what the conventions are.
const HISTORY: usize = 60;
/// The most commits "by context" proposes: more would be noise.
const MAX_GROUPS: usize = 8;
/// A subject longer than this is refused whatever the repository does.
const HARD_SUBJECT_LIMIT: usize = 100;
const DIFF_BUDGET: usize = 20_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    Conventional,
    Gitmoji,
    Free,
}

/// What the history of a repository says about how its commits are written.
#[derive(Clone, Debug, PartialEq)]
pub struct Convention {
    pub style: Style,
    pub french: bool,
    pub types: Vec<(String, usize)>,
    pub scopes: Vec<(String, usize)>,
    /// Share of the commits that have a scope.
    pub scope_rate: f32,
    /// Whether the description starts with a capital.
    pub capitalized: bool,
    pub period: bool,
    pub max_subject: usize,
    pub body_rate: f32,
    pub examples: Vec<String>,
    pub samples: usize,
}

fn conventional() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(?P<type>[A-Za-z]+)(?:\((?P<scope>[^)]+)\))?(?P<bang>!)?: (?P<desc>\S.*)$").expect("a valid expression"))
}

fn is_gitmoji(subject: &str) -> bool {
    let first = subject.chars().next();
    subject.starts_with(':') && subject[1..].split(':').next().is_some_and(|c| !c.is_empty() && c.chars().all(|ch| ch.is_alphanumeric() || ch == '_'))
        || first.is_some_and(|c| (c as u32) >= 0x2190 && !c.is_alphabetic())
}

/// Whether a line is French, from the little words and the verbs commit messages start with.
fn french_score(subject: &str) -> i32 {
    const FRENCH: [&str; 14] = ["le", "la", "les", "des", "du", "un", "une", "pour", "dans", "avec", "sans", "au", "aux", "et"];
    const ENGLISH: [&str; 11] = ["the", "a", "an", "of", "for", "to", "with", "in", "on", "and", "from"];
    const FRENCH_VERBS: [&str; 16] = [
        "ajoute", "corrige", "configure", "supprime", "met", "rend", "remplace", "retire", "crée", "ajout", "correction", "mise", "fusionne", "renomme", "déplace", "initialise",
    ];
    const ENGLISH_VERBS: [&str; 14] = ["add", "fix", "update", "remove", "refactor", "rename", "move", "initialize", "implement", "improve", "create", "bump", "merge", "use"];
    let lower = subject.to_lowercase();
    // What follows a type prefix is the description.
    let description = conventional().captures(&lower).and_then(|c| c.name("desc")).map_or(lower.as_str(), |d| d.as_str());
    let words: Vec<&str> = description.split(|c: char| !c.is_alphabetic() && c != 'é' && c != 'è').filter(|w| !w.is_empty()).collect();
    let mut score = 0;
    for (i, word) in words.iter().enumerate() {
        if FRENCH.contains(word) {
            score += 1;
        }
        if ENGLISH.contains(word) {
            score -= 1;
        }
        if i == 0 && FRENCH_VERBS.contains(word) {
            score += 3;
        }
        if i == 0 && ENGLISH_VERBS.contains(word) {
            score -= 3;
        }
    }
    score
}

fn ranked(counts: HashMap<String, usize>) -> Vec<(String, usize)> {
    let mut list: Vec<(String, usize)> = counts.into_iter().collect();
    list.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    list
}

/// The conventions read from the messages of the latest commits (subject first, then the body).
pub fn detect_from(messages: &[String]) -> Convention {
    let subjects: Vec<&str> = messages.iter().filter_map(|m| m.lines().next()).map(str::trim).filter(|s| !s.is_empty()).collect();
    let samples = subjects.len();
    let share = |count: usize| if samples == 0 { 0.0 } else { count as f32 / samples as f32 };
    let parsed: Vec<_> = subjects.iter().filter_map(|s| conventional().captures(s)).collect();
    let moji = subjects.iter().filter(|s| is_gitmoji(s)).count();
    let style = if share(parsed.len()) >= 0.6 {
        Style::Conventional
    } else if share(moji) >= 0.6 {
        Style::Gitmoji
    } else {
        Style::Free
    };
    let mut types: HashMap<String, usize> = HashMap::new();
    let mut scopes: HashMap<String, usize> = HashMap::new();
    let mut with_scope = 0;
    for c in &parsed {
        *types.entry(c["type"].to_lowercase()).or_default() += 1;
        if let Some(scope) = c.name("scope") {
            *scopes.entry(scope.as_str().to_string()).or_default() += 1;
            with_scope += 1;
        }
    }
    let descriptions: Vec<&str> = match style {
        Style::Conventional => parsed.iter().filter_map(|c| c.name("desc")).map(|d| d.as_str()).collect(),
        _ => subjects.clone(),
    };
    let capitalized = descriptions.iter().filter(|d| d.chars().next().is_some_and(char::is_uppercase)).count() * 2 > descriptions.len();
    let period = subjects.iter().filter(|s| s.ends_with('.')).count() * 2 > samples;
    let with_body = messages.iter().filter(|m| m.lines().skip(1).any(|l| !l.trim().is_empty())).count();
    Convention {
        style,
        french: subjects.iter().map(|s| french_score(s)).sum::<i32>() > 0,
        types: ranked(types),
        scopes: ranked(scopes),
        scope_rate: if parsed.is_empty() { 0.0 } else { with_scope as f32 / parsed.len() as f32 },
        capitalized,
        period,
        max_subject: subjects.iter().map(|s| s.chars().count()).max().unwrap_or(0),
        body_rate: share(with_body),
        examples: subjects.iter().take(5).map(|s| s.to_string()).collect(),
        samples,
    }
}

/// The conventions of a repository, from its history; the defaults of a new one.
pub fn detect(repo: &Repo) -> Convention {
    let out = repo.read(&["log", "--no-merges", &format!("-n{HISTORY}"), "--format=%B%x1e"]).unwrap_or_default();
    let messages: Vec<String> = out.split('\u{1e}').map(|m| m.trim().to_string()).filter(|m| !m.is_empty()).collect();
    detect_from(&messages)
}

/// What the AI is told about how to write the message.
pub fn guidance(c: &Convention) -> String {
    let language = if c.french { "French" } else { "English" };
    let mut lines = Vec::new();
    match c.style {
        Style::Conventional => {
            let types: Vec<String> = c.types.iter().take(8).map(|(t, n)| format!("{t} ({n})")).collect();
            lines.push(format!("Use Conventional Commits: `type(scope): description`. Types used in this repository: {}.", types.join(", ")));
            if c.scope_rate >= 0.5 {
                let scopes: Vec<&str> = c.scopes.iter().take(8).map(|(s, _)| s.as_str()).collect();
                lines.push(format!("Most commits have a scope ({:.0}%): {}.", c.scope_rate * 100.0, scopes.join(", ")));
            } else if c.scope_rate > 0.0 {
                lines.push(format!("A scope is used in only {:.0}% of the commits: add one only when it clarifies.", c.scope_rate * 100.0));
            } else {
                lines.push("This repository does not use scopes: write `type: description`.".into());
            }
        }
        Style::Gitmoji => lines.push("Start the subject with a gitmoji, as the repository's commits do.".into()),
        Style::Free => lines.push("Commits here are free text, not Conventional Commits: do not add a `type:` prefix.".into()),
    }
    lines.push(format!(
        "Write the subject in {language}, {} first letter, {} trailing period, at most {} characters.",
        if c.capitalized { "capital" } else { "lowercase" },
        if c.period { "with a" } else { "no" },
        c.max_subject.clamp(50, HARD_SUBJECT_LIMIT)
    ));
    lines.push(if c.body_rate >= 0.4 {
        "A body is common here: add one (after a blank line) to say why when it is not obvious.".into()
    } else {
        "Bodies are rare here: add one only when the why is not obvious.".into()
    });
    lines.push("Never add a Co-authored-by line or any attribution to an AI; they are removed anyway.".into());
    lines.join("\n")
}

/// Removes what must never be in a commit: co-author lines and attributions to an AI. The rest of
/// the message is kept as written, minus the blank lines left at the end.
pub fn clean_message(message: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let attribution = RE.get_or_init(|| {
        Regex::new(r"(?i)^\s*(co-authored-by\s*:|(🤖\s*)?generated\s+(with|by)\b.*(claude|codex|gemini|chatgpt|copilot|openai|anthropic|\bai\b))").expect("a valid expression")
    });
    let text = message.replace("\r\n", "\n");
    let kept: Vec<&str> = text.lines().filter(|l| !attribution.is_match(l)).collect();
    kept.join("\n").trim().to_string()
}

/// Whether a message follows the conventions, or why not.
pub fn check(c: &Convention, message: &str) -> Result<(), String> {
    let subject = message.lines().next().unwrap_or("").trim();
    if subject.is_empty() {
        return Err("the commit message is empty".into());
    }
    if subject.chars().count() > HARD_SUBJECT_LIMIT {
        return Err(format!("the subject is {} characters long: keep it under {HARD_SUBJECT_LIMIT}", subject.chars().count()));
    }
    match c.style {
        Style::Conventional => match conventional().captures(subject) {
            None => Err(format!("“{subject}” is not a Conventional Commit: this repository writes `type(scope): description`. {}", c.examples.first().map_or(String::new(), |e| format!("For example: {e}")))),
            Some(found) => {
                let kind = found["type"].to_lowercase();
                const STANDARD: [&str; 11] = ["feat", "fix", "docs", "style", "refactor", "perf", "test", "build", "ci", "chore", "revert"];
                if !c.types.iter().any(|(t, _)| *t == kind) && !STANDARD.contains(&kind.as_str()) {
                    return Err(format!("the type “{kind}” is not used in this repository: use one of {}", c.types.iter().map(|(t, _)| t.as_str()).collect::<Vec<_>>().join(", ")));
                }
                Ok(())
            }
        },
        Style::Gitmoji if !is_gitmoji(subject) => Err("this repository starts its subjects with a gitmoji".into()),
        Style::Free if conventional().is_match(subject) && c.samples >= 10 => {
            Err("this repository does not use `type: description` prefixes: write a plain sentence".into())
        }
        _ => Ok(()),
    }
}

// ---- Changes and how to split them ---------------------------------------------------------

fn status_name(kind: Kind) -> &'static str {
    match kind {
        Kind::Modified => "modified",
        Kind::Added => "added",
        Kind::Deleted => "deleted",
        Kind::Renamed => "renamed",
        Kind::Untracked => "untracked",
        Kind::Conflicted => "conflicted",
    }
}

/// What kind of change a file is part of, apart from the code.
fn bucket(path: &str) -> &'static str {
    let lower = path.to_lowercase();
    let name = lower.rsplit('/').next().unwrap_or("");
    let extension = name.rsplit_once('.').map_or("", |(_, e)| e);
    if lower.starts_with(".github/") || lower.starts_with(".gitlab") || matches!(name, "dockerfile" | ".gitignore" | ".gitattributes" | ".editorconfig" | "dependabot.yml") {
        "ci"
    } else if matches!(name, "cargo.lock" | "cargo.toml" | "package.json" | "package-lock.json" | "pnpm-lock.yaml" | "yarn.lock" | "go.mod" | "go.sum" | "requirements.txt" | "poetry.lock" | "gemfile" | "gemfile.lock") {
        "deps"
    } else if matches!(extension, "md" | "mdx" | "rst") || lower.starts_with("docs/") || lower.starts_with("doc/") || matches!(name, "license" | "changelog" | "readme") {
        "docs"
    } else {
        "code"
    }
}

#[derive(Clone, Debug)]
pub struct Group {
    pub title: String,
    pub kind: &'static str,
    pub files: Vec<Change>,
}

fn directory(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(d, _)| d)
}

/// The folder all of `paths` are in (possibly the project root, as an empty string).
fn common_directory(paths: &[&str]) -> String {
    let mut common: Vec<&str> = match paths.first() {
        Some(first) => directory(first).split('/').filter(|p| !p.is_empty()).collect(),
        None => return String::new(),
    };
    for path in &paths[1..] {
        let parts: Vec<&str> = directory(path).split('/').filter(|p| !p.is_empty()).collect();
        let shared = common.iter().zip(&parts).take_while(|(a, b)| a == b).count();
        common.truncate(shared);
    }
    common.join("/")
}

fn describe_group(files: &[Change]) -> String {
    let paths: Vec<&str> = files.iter().map(|c| c.path.as_str()).collect();
    let dir = common_directory(&paths);
    match (files.len(), dir.is_empty()) {
        (1, _) => files[0].path.clone(),
        (n, true) => format!("{n} files at the root and across folders"),
        (n, false) => format!("{dir} ({n} files)"),
    }
}

/// Splits the changes into coherent commits. `connected` says which pairs of changed files lean
/// on each other (from the context graph), by index into `changes`.
pub fn group(changes: &[Change], connected: &[(usize, usize)]) -> Vec<Group> {
    let mut groups: Vec<Group> = Vec::new();
    for kind in ["docs", "deps", "ci"] {
        let files: Vec<Change> = changes.iter().filter(|c| bucket(&c.path) == kind).cloned().collect();
        if !files.is_empty() {
            groups.push(Group { title: describe_group(&files), kind, files });
        }
    }
    let code: Vec<usize> = (0..changes.len()).filter(|&i| bucket(&changes[i].path) == "code").collect();
    // Files that lean on each other, and files of the same folder, make one change.
    let mut parent: Vec<usize> = (0..changes.len()).collect();
    fn find(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    let in_code: HashSet<usize> = code.iter().copied().collect();
    for &(a, b) in connected.iter().filter(|(a, b)| in_code.contains(a) && in_code.contains(b)) {
        let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
        parent[ra.max(rb)] = ra.min(rb);
    }
    let mut by_dir: HashMap<&str, usize> = HashMap::new();
    for &i in &code {
        let first = *by_dir.entry(directory(&changes[i].path)).or_insert(i);
        let (ra, rb) = (find(&mut parent, first), find(&mut parent, i));
        parent[ra.max(rb)] = ra.min(rb);
    }
    let mut components: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for &i in &code {
        let root = find(&mut parent, i);
        components.entry(root).or_default().push(i);
    }
    let mut code_groups: Vec<Vec<usize>> = components.into_values().collect();
    // Too many small changes are noise: the smallest goes with the one it shares most folders with.
    while code_groups.len() + groups.len() > MAX_GROUPS && code_groups.len() > 1 {
        code_groups.sort_by_key(|g| std::cmp::Reverse(g.len()));
        let smallest = code_groups.pop().unwrap_or_default();
        let dir_of = |g: &Vec<usize>| common_directory(&g.iter().map(|&i| changes[i].path.as_str()).collect::<Vec<_>>());
        let wanted = dir_of(&smallest);
        let best = (0..code_groups.len())
            .max_by_key(|&k| {
                let other = dir_of(&code_groups[k]);
                wanted.split('/').zip(other.split('/')).take_while(|(a, b)| a == b && !a.is_empty()).count()
            })
            .unwrap_or(0);
        code_groups[best].extend(smallest);
    }
    code_groups.sort_by_key(|g| g[0]);
    let mut made: Vec<Group> = code_groups
        .into_iter()
        .map(|g| {
            let files: Vec<Change> = g.into_iter().map(|i| changes[i].clone()).collect();
            Group { title: describe_group(&files), kind: "code", files }
        })
        .collect();
    made.append(&mut groups);
    made
}

/// The hint of a type for a group, when the files say it.
fn suggested_type(g: &Group) -> Option<&'static str> {
    match g.kind {
        "docs" => Some("docs"),
        "deps" => Some("build or chore"),
        "ci" => Some("ci"),
        _ if g.files.iter().all(|c| matches!(c.kind, Kind::Added | Kind::Untracked)) => Some("feat"),
        _ => None,
    }
}

/// Lines added and removed per file, for what git knows about (not the new files).
fn numstat(repo: &Repo) -> HashMap<String, (u64, u64)> {
    let out = repo.read(&["diff", "HEAD", "--numstat", "--no-renames"]).unwrap_or_default();
    out.lines()
        .filter_map(|line| {
            let mut parts = line.splitn(3, '\t');
            let (added, deleted, path) = (parts.next()?, parts.next()?, parts.next()?);
            Some((path.to_string(), (added.parse().unwrap_or(0), deleted.parse().unwrap_or(0))))
        })
        .collect()
}

fn file_json(change: &Change, stats: &HashMap<String, (u64, u64)>, repo: &Repo) -> Value {
    let (added, deleted) = stats.get(&change.path).copied().unwrap_or_else(|| {
        // A new file has no diff yet: its lines are what it adds.
        let lines = std::fs::read_to_string(repo.abs(&change.path)).map_or(0, |t| t.lines().count() as u64);
        (if change.kind == Kind::Untracked { lines } else { 0 }, 0)
    });
    json!({"path": change.path, "status": status_name(change.kind), "renamedFrom": change.orig, "added": added, "deleted": deleted})
}

/// The pairs of changed files the context graph links, by index into `changes`.
fn graph_links(changes: &[Change], repo: &Repo, root: &Path) -> Vec<(usize, usize)> {
    let Some(graph) = crate::graph::current() else { return Vec::new() };
    let project = crate::project::simplify(root);
    let position: HashMap<String, usize> = changes
        .iter()
        .enumerate()
        .filter_map(|(i, c)| {
            let abs = crate::project::simplify(&repo.abs(&c.path));
            let rel = abs.strip_prefix(&project).ok()?.to_string_lossy().replace('\\', "/");
            Some((rel, i))
        })
        .collect();
    let file_of = |node: usize| position.get(&graph.nodes[node].file).copied();
    graph
        .links
        .iter()
        .filter(|l| l.kind != "contains")
        .filter_map(|l| Some((file_of(l.source)?, file_of(l.target)?)))
        .filter(|(a, b)| a != b)
        .collect()
}

fn open(root: &Path) -> Result<Repo, String> {
    match Repo::open(root, Console::default()) {
        Ok(Some(repo)) => Ok(repo),
        Ok(None) => Err("This project is not a git repository".into()),
        Err(e) => Err(format!("{e}")),
    }
}

fn mode_of(args: &Value) -> Result<&str, String> {
    match args["mode"].as_str() {
        Some(mode @ ("all" | "context")) => Ok(mode),
        Some(other) => Err(format!("“mode” is “all” or “context”, not “{other}”")),
        None => Err("“mode” is required: “all” (one commit) or “context” (one commit per coherent change)".into()),
    }
}

/// `git_commit_plan`: the conventions, the changes and how to split them.
pub fn plan(root: &Path, args: &Value) -> Result<String, String> {
    let mode = mode_of(args)?;
    let repo = open(root)?;
    let (head, changes) = repo.status().map_err(|e| e.to_string())?;
    if changes.is_empty() {
        return Err("Nothing to commit: the working tree is clean".into());
    }
    let convention = detect(&repo);
    let stats = numstat(&repo);
    let groups = if mode == "all" {
        vec![Group { title: describe_group(&changes), kind: "all", files: changes.clone() }]
    } else {
        group(&changes, &graph_links(&changes, &repo, root))
    };
    let files = |g: &Group| g.files.iter().map(|c| file_json(c, &stats, &repo)).collect::<Vec<_>>();
    serde_json::to_string_pretty(&json!({
        "mode": mode,
        "branch": head.branch,
        "conventions": {
            "style": match convention.style { Style::Conventional => "conventional", Style::Gitmoji => "gitmoji", Style::Free => "free" },
            "language": if convention.french { "fr" } else { "en" },
            "types": convention.types.iter().map(|(t, n)| json!({"type": t, "commits": n})).collect::<Vec<_>>(),
            "scopes": convention.scopes.iter().take(12).map(|(s, n)| json!({"scope": s, "commits": n})).collect::<Vec<_>>(),
            "examples": convention.examples,
            "basedOnCommits": convention.samples,
        },
        "guidance": guidance(&convention),
        "groups": groups.iter().enumerate().map(|(i, g)| json!({
            "id": i + 1,
            "title": g.title,
            "kind": g.kind,
            "suggestedType": suggested_type(g),
            "files": files(g),
        })).collect::<Vec<_>>(),
        "next": if mode == "all" {
            "Read the changes (git_diff or your own tools), write ONE message following `guidance`, then call git_commit with mode \"all\" and commits: [{\"message\": ...}]."
        } else {
            "Read the changes (git_diff or your own tools), write one message per group following `guidance`, then call git_commit with mode \"context\" and commits: [{\"files\": [...the group's paths...], \"message\": ...}]. You may merge or split groups if the changes call for it."
        },
    }))
    .map_err(|e| e.to_string())
}

/// `git_diff`: what changed, to write the message from.
pub fn diff(root: &Path, args: &Value) -> Result<String, String> {
    let repo = open(root)?;
    let (_, changes) = repo.status().map_err(|e| e.to_string())?;
    let wanted: Vec<String> = args["files"].as_array().map(|a| a.iter().filter_map(|f| f.as_str().map(str::to_string)).collect()).unwrap_or_default();
    let selected: Vec<&Change> = changes.iter().filter(|c| wanted.is_empty() || wanted.contains(&c.path)).collect();
    let budget = args["maxChars"].as_u64().map_or(DIFF_BUDGET, |n| n as usize).clamp(1_000, 100_000);
    let mut out = String::new();
    for change in selected {
        let section = if change.kind == Kind::Untracked {
            let text = std::fs::read_to_string(repo.abs(&change.path)).unwrap_or_else(|_| "(binary or unreadable file)".into());
            let shown: String = text.lines().take(200).map(|l| format!("+{l}\n")).collect();
            format!("new file: {}\n{shown}\n", change.path)
        } else {
            repo.read(&["diff", "HEAD", "--no-color", "--unified=3", "--", &change.path]).unwrap_or_default()
        };
        if out.len() + section.len() > budget {
            let mut cut = budget.saturating_sub(out.len()).min(section.len());
            while !section.is_char_boundary(cut) {
                cut -= 1;
            }
            out.push_str(&section[..cut]);
            out.push_str("\n… diff cut: ask for fewer files or raise maxChars.\n");
            break;
        }
        out.push_str(&section);
    }
    if out.is_empty() { Err("No such changes".into()) } else { Ok(out) }
}

/// One commit to make.
#[derive(Debug)]
pub struct Request {
    pub message: String,
    /// None for "all".
    pub files: Option<Vec<String>>,
}

fn requests(args: &Value) -> Result<Vec<Request>, String> {
    let list = args["commits"].as_array().filter(|a| !a.is_empty()).ok_or("“commits” is required: a list of {message, files}")?;
    list.iter()
        .map(|c| {
            Ok(Request {
                message: c["message"].as_str().ok_or("each commit needs a “message”")?.to_string(),
                files: c["files"].as_array().map(|f| f.iter().filter_map(|p| p.as_str().map(str::to_string)).collect()),
            })
        })
        .collect()
}

/// Makes the commits, after checking all of them: either every message and file is acceptable or
/// nothing is committed. The summary of what was done.
pub fn commit(root: &Path, args: &Value) -> Result<String, String> {
    let mode = mode_of(args)?;
    let asked = requests(args)?;
    let repo = open(root)?;
    if repo.state() != State::Clean {
        return Err("A merge, rebase, cherry-pick or revert is in progress: finish it first".into());
    }
    let (_, changes) = repo.status().map_err(|e| e.to_string())?;
    if changes.is_empty() {
        return Err("Nothing to commit: the working tree is clean".into());
    }
    if changes.iter().any(|c| c.kind == Kind::Conflicted) {
        return Err("Some files have merge conflicts: resolve them first".into());
    }
    let convention = detect(&repo);
    let mut commits: Vec<(String, Option<Vec<String>>)> = Vec::new();
    let mut used: HashSet<String> = HashSet::new();
    for (n, request) in asked.iter().enumerate() {
        let message = clean_message(&request.message);
        check(&convention, &message).map_err(|e| format!("commit {}: {e}", n + 1))?;
        let files = match (mode, &request.files) {
            ("all", _) => None,
            (_, Some(files)) if !files.is_empty() => {
                let mut paths: Vec<String> = Vec::new();
                for file in files {
                    let change = changes.iter().find(|c| c.path == *file).ok_or_else(|| format!("commit {}: “{file}” has no changes", n + 1))?;
                    if !used.insert(file.clone()) {
                        return Err(format!("“{file}” is in two commits"));
                    }
                    paths.push(file.clone());
                    // A rename is the old path going and the new one coming.
                    paths.extend(change.orig.clone());
                }
                Some(paths)
            }
            _ => return Err(format!("commit {}: “files” is required in “context” mode", n + 1)),
        };
        commits.push((message, files));
    }
    if mode == "all" && commits.len() != 1 {
        return Err("“all” makes exactly one commit".into());
    }
    let mut done = Vec::new();
    for (message, files) in &commits {
        let mut paragraphs = message.split("\n\n").map(str::trim).filter(|p| !p.is_empty());
        let mut args: Vec<String> = vec!["commit".into()];
        for paragraph in paragraphs.by_ref() {
            args.push("-m".into());
            args.push(paragraph.to_string());
        }
        let arguments: Vec<&str> = args.iter().map(String::as_str).collect();
        let outcome = match files {
            None => repo.run(&["add", "-A"]).and_then(|_| repo.run(&arguments)),
            Some(paths) => repo.run_paths(&["add", "-A"], paths).and_then(|_| repo.run_paths(&arguments, paths)),
        };
        match outcome {
            Ok(_) => {
                let head = repo.read(&["log", "-1", "--format=%h %s"]).unwrap_or_default();
                done.push(head.trim().to_string());
            }
            Err(e) => {
                let so_far = if done.is_empty() { String::new() } else { format!(" Already made: {}.", done.join("; ")) };
                return Err(format!("git commit failed: {e}.{so_far}"));
            }
        }
    }
    let left = repo.status().map(|(_, c)| c.len()).unwrap_or(0);
    Ok(format!(
        "{} commit(s) made:\n{}\n{}",
        done.len(),
        done.iter().map(|d| format!("- {d}")).collect::<Vec<_>>().join("\n"),
        if left == 0 { "The working tree is clean.".to_string() } else { format!("{left} changed file(s) remain uncommitted.") }
    ))
}

/// The tools of this module, for the server. None for another tool.
pub fn tool(name: &str, args: &Value, root: &Path) -> Option<Result<String, String>> {
    Some(match name {
        "git_commit_plan" => plan(root, args),
        "git_diff" => diff(root, args),
        "git_commit" => commit(root, args),
        _ => return None,
    })
}

/// The commands Atelier offers to the client (MCP prompts, which AI tools list as slash
/// commands): `commit`, with the mode as an argument, and its two shortcuts.
pub fn prompts() -> Vec<Value> {
    vec![
        json!({
            "name": "commit",
            "title": "Commit",
            "description": "Commit the changes of the project, following the repository's conventions: all at once, or one commit per context.",
            "arguments": [{"name": "mode", "description": "“all” (one commit) or “context” (one commit per coherent change). Asked if missing.", "required": false}],
        }),
        json!({"name": "commit-all", "title": "Commit all", "description": "One commit with all the changes.", "arguments": []}),
        json!({"name": "commit-by-context", "title": "Commit by context", "description": "One commit per coherent change (files that go together, docs, dependencies, CI).", "arguments": []}),
    ]
}

/// The text of a command of [`prompts`].
pub fn prompt(name: &str, args: &Value) -> Option<String> {
    let mode = match name {
        "commit-all" => Some("all"),
        "commit-by-context" => Some("context"),
        "commit" => args["mode"].as_str().filter(|m| matches!(*m, "all" | "context")),
        _ => return None,
    };
    let start = match mode {
        Some(mode) => format!("Commit the current changes of this project in “{mode}” mode."),
        None => "Commit the current changes of this project. First ask the user which mode they want: “all” (one commit with everything) or “context” (one commit per coherent change). Then continue with their answer.".into(),
    };
    Some(format!(
        "{start}\n\n\
         1. Call the Atelier tool `git_commit_plan` with the mode. It returns the repository's commit conventions (style, language, types, scopes, length) and the files to commit, already grouped in “context” mode.\n\
         2. Read what changed (`git_diff`, or your own tools) and write the message of each commit from the changes, following the returned `guidance` exactly. Keep the changes of one group together; you may merge or split groups when the changes call for it.\n\
         3. Call `git_commit` with the mode and the list of commits ({{message, files}}). Atelier checks every message against the conventions first and commits nothing if one does not follow them.\n\n\
         Rules: never add a `Co-authored-by` line or any attribution to an AI in a message; do not push; do not amend or rewrite existing commits; tell the user what was committed."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{path::PathBuf, process::Command};

    fn git(dir: &Path, args: &[&str]) {
        let out = Command::new("git").current_dir(dir).args(args).output().expect("git is installed");
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    }

    fn repo_with(name: &str, history: &[&str]) -> PathBuf {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("atelier-commits-{name}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        git(&dir, &["init", "-q"]);
        git(&dir, &["config", "user.email", "moi@example.com"]);
        git(&dir, &["config", "user.name", "Moi"]);
        git(&dir, &["config", "commit.gpgsign", "false"]);
        for (i, subject) in history.iter().enumerate() {
            std::fs::write(dir.join(format!("h{i}.txt")), i.to_string()).unwrap();
            git(&dir, &["add", "-A"]);
            git(&dir, &["commit", "-q", "-m", subject]);
        }
        dir
    }

    fn subjects(dir: &Path) -> Vec<String> {
        let out = Command::new("git").current_dir(dir).args(["log", "--format=%B%x1e"]).output().unwrap();
        String::from_utf8_lossy(&out.stdout).split('\u{1e}').map(|m| m.trim().to_string()).filter(|m| !m.is_empty()).collect()
    }

    #[test]
    fn conventions_are_read_from_the_history() {
        let french: Vec<String> = [
            "feat(editor): Ajoute le repli du code",
            "fix(terminal): Corrige le fractionnement",
            "docs: Met à jour le README",
            "feat(graph): Ajoute le graphe de contexte",
            "chore: Configure le dépôt",
        ]
        .map(String::from)
        .to_vec();
        let c = detect_from(&french);
        assert_eq!((c.style, c.french, c.capitalized, c.period), (Style::Conventional, true, true, false));
        assert_eq!(c.types[0], ("feat".to_string(), 2));
        assert!(c.scope_rate > 0.5 && c.scopes.iter().any(|(s, _)| s == "editor"));
        let english: Vec<String> = ["Add folding", "Fix the terminal split", "Update the readme", "Remove dead code"].map(String::from).to_vec();
        let c = detect_from(&english);
        assert_eq!((c.style, c.french), (Style::Free, false));
        let moji: Vec<String> = ["✨ add folding", "🐛 fix split", "📝 docs", "🎨 style"].map(String::from).to_vec();
        assert_eq!(detect_from(&moji).style, Style::Gitmoji);
        assert_eq!(detect_from(&[]).samples, 0);
        assert!(guidance(&detect_from(&french)).contains("Conventional Commits") && guidance(&detect_from(&french)).contains("French"));
    }

    #[test]
    fn messages_are_checked_against_the_conventions_of_the_repository() {
        let conventional = detect_from(&["feat(a): x".to_string(), "fix: y".to_string(), "docs: z".to_string()]);
        assert!(check(&conventional, "feat(ui): add a button").is_ok());
        assert!(check(&conventional, "Add a button").unwrap_err().contains("not a Conventional Commit"));
        assert!(check(&conventional, "wibble: add a button").unwrap_err().contains("not used"));
        assert!(check(&conventional, &format!("feat: {}", "x".repeat(120))).unwrap_err().contains("characters"));
        assert!(check(&conventional, "  ").is_err());
        let free = detect_from(&(0..12).map(|i| format!("Add thing {i}")).collect::<Vec<_>>());
        assert!(check(&free, "Add another thing").is_ok());
        assert!(check(&free, "feat: add another thing").unwrap_err().contains("plain sentence"));
    }

    #[test]
    fn co_authors_and_ai_attributions_never_stay_in_a_message() {
        let message = "feat: ajoute x\n\nLe détail.\n\nCo-Authored-By: Claude Sonnet <noreply@anthropic.com>\n🤖 Generated with [Claude Code](https://claude.com/claude-code)\nco-authored-by: Autre <a@b.c>\n";
        assert_eq!(clean_message(message), "feat: ajoute x\n\nLe détail.");
        assert_eq!(clean_message("fix: y\r\n\r\nCo-authored-by: x <y@z>"), "fix: y");
        assert_eq!(clean_message("docs: explain how a Generated with caching works"), "docs: explain how a Generated with caching works", "ordinary text is left alone");
    }

    fn change(path: &str, kind: Kind) -> Change {
        Change { path: path.into(), orig: None, kind }
    }

    #[test]
    fn changes_are_split_by_context() {
        let changes = vec![
            change("src/graph/mod.rs", Kind::Modified),
            change("src/graph/build.rs", Kind::Modified),
            change("src/editor.rs", Kind::Modified),
            change("src/ui.rs", Kind::Modified),
            change("README.md", Kind::Modified),
            change("Cargo.lock", Kind::Modified),
            change(".github/workflows/ci.yml", Kind::Added),
        ];
        // editor.rs and ui.rs lean on each other (from the graph); the graph folder goes together.
        let groups = group(&changes, &[(2, 3)]);
        let by_kind = |k: &str| groups.iter().filter(|g| g.kind == k).count();
        assert_eq!((by_kind("docs"), by_kind("deps"), by_kind("ci")), (1, 1, 1));
        let code: Vec<Vec<&str>> = groups.iter().filter(|g| g.kind == "code").map(|g| g.files.iter().map(|c| c.path.as_str()).collect()).collect();
        assert_eq!(code.len(), 2, "{code:?}");
        assert!(code.iter().any(|g| g == &["src/graph/mod.rs", "src/graph/build.rs"]));
        assert!(code.iter().any(|g| g.contains(&"src/editor.rs") && g.contains(&"src/ui.rs")), "linked files stay together: {code:?}");
        // Every file is in exactly one group.
        assert_eq!(groups.iter().map(|g| g.files.len()).sum::<usize>(), changes.len());
        // Many folders do not make many commits.
        let many: Vec<Change> = (0..20).map(|i| change(&format!("dir{i}/f.rs"), Kind::Modified)).collect();
        assert!(group(&many, &[]).len() <= MAX_GROUPS);
        assert_eq!(suggested_type(&Group { title: String::new(), kind: "code", files: vec![change("a", Kind::Untracked)] }), Some("feat"));
    }

    #[test]
    fn everything_is_committed_at_once_without_a_co_author() {
        let dir = repo_with("all", &["feat: un", "fix: deux", "docs: trois"]);
        std::fs::write(dir.join("new.rs"), "fn x() {}").unwrap();
        std::fs::write(dir.join("h0.txt"), "changed").unwrap();
        let plan_text = plan(&dir, &json!({"mode": "all"})).unwrap();
        let plan: Value = serde_json::from_str(&plan_text).unwrap();
        assert_eq!(plan["conventions"]["style"], "conventional");
        assert_eq!(plan["groups"].as_array().unwrap().len(), 1);
        assert_eq!(plan["groups"][0]["files"].as_array().unwrap().len(), 2);
        // A message that does not follow the repository is refused and nothing is committed.
        let refused = commit(&dir, &json!({"mode": "all", "commits": [{"message": "Add stuff"}]})).unwrap_err();
        assert!(refused.contains("Conventional Commit"), "{refused}");
        assert_eq!(subjects(&dir).len(), 3);
        let done = commit(&dir, &json!({"mode": "all", "commits": [{"message": "feat: add new.rs\n\nCo-Authored-By: Claude <noreply@anthropic.com>"}]})).unwrap();
        assert!(done.contains("1 commit(s) made") && done.contains("clean"), "{done}");
        let log = subjects(&dir);
        assert_eq!(log[0], "feat: add new.rs", "{log:?}");
        assert!(!log[0].to_lowercase().contains("co-authored"));
        assert!(plan_nothing(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn plan_nothing(dir: &Path) -> bool {
        plan(dir, &json!({"mode": "all"})).unwrap_err().contains("Nothing to commit")
    }

    #[test]
    fn by_context_makes_one_commit_per_group_and_leaves_the_rest() {
        let dir = repo_with("context", &["feat: un", "fix: deux", "docs: trois"]);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/a.rs"), "a").unwrap();
        std::fs::write(dir.join("src/b.rs"), "b").unwrap();
        std::fs::write(dir.join("README.md"), "docs").unwrap();
        std::fs::write(dir.join("h1.txt"), "left alone").unwrap();
        let groups: Value = serde_json::from_str(&plan(&dir, &json!({"mode": "context"})).unwrap()).unwrap();
        let kinds: Vec<&str> = groups["groups"].as_array().unwrap().iter().filter_map(|g| g["kind"].as_str()).collect();
        assert!(kinds.contains(&"docs") && kinds.contains(&"code"), "{kinds:?}");
        // Missing files, unknown files and a file in two commits are all refused up front.
        for bad in [
            json!([{"message": "feat: x"}]),
            json!([{"message": "feat: x", "files": ["nope.rs"]}]),
            json!([{"message": "feat: x", "files": ["src/a.rs"]}, {"message": "fix: y", "files": ["src/a.rs"]}]),
        ] {
            assert!(commit(&dir, &json!({"mode": "context", "commits": bad})).is_err());
        }
        assert_eq!(subjects(&dir).len(), 3, "nothing was committed by the refused calls");
        let done = commit(&dir, &json!({"mode": "context", "commits": [
            {"message": "feat(src): add a and b", "files": ["src/a.rs", "src/b.rs"]},
            {"message": "docs: describe the change", "files": ["README.md"]},
        ]}))
        .unwrap();
        assert!(done.contains("2 commit(s) made") && done.contains("1 changed file(s) remain"), "{done}");
        let log = subjects(&dir);
        assert_eq!(&log[..2], ["docs: describe the change", "feat(src): add a and b"]);
        let status = Command::new("git").current_dir(&dir).args(["status", "--porcelain"]).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&status.stdout).trim(), "M h1.txt", "only the file nobody listed is left");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_commit_command_is_offered_with_its_two_modes() {
        let names: Vec<String> = prompts().iter().filter_map(|p| p["name"].as_str().map(str::to_string)).collect();
        assert_eq!(names, ["commit", "commit-all", "commit-by-context"]);
        assert!(prompt("commit-all", &json!({})).unwrap().contains("“all” mode"));
        assert!(prompt("commit", &json!({"mode": "context"})).unwrap().contains("“context” mode"));
        assert!(prompt("commit", &json!({})).unwrap().contains("ask the user which mode"));
        let text = prompt("commit-by-context", &json!({})).unwrap();
        assert!(text.contains("never add a `Co-authored-by`") && text.contains("git_commit_plan") && text.contains("git_commit"));
        assert_eq!(prompt("autre", &json!({})), None);
    }
}
