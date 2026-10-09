//! `rework-commit`: the AI reviews the messages of the latest commits and rewrites the ones that
//! are wrong. Atelier does the part that does not need judgement:
//!
//! - `git_rework_plan` reads the commits (subject, body, what each one changes) and flags what does
//!   not match: a subject that breaks the repository's conventions, a type that contradicts the
//!   files (`docs:` on code), a scope found in no changed path, a body that names files the commit
//!   does not touch, a co-author or an AI attribution, a big change with no description;
//! - `git_rework_apply` rewrites the messages of the commits the AI chose.
//!
//! Rewriting history is not done lightly: the commits must be a straight line (no merge), none may
//! be already on a remote unless the user said so, the working tree must be clean, and the old
//! head is kept as a ref before anything moves. Only messages change: trees, authors and dates are
//! carried over as they were, so no file is touched and no conflict can happen.

use super::commits::{self, Convention, Style};
use crate::plugins::builtins::git::repo::{Console, Repo};
use regex::Regex;
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    path::Path,
    sync::OnceLock,
    time::{SystemTime, UNIX_EPOCH},
};

/// Most commits one rework looks at.
const MAX_COMMITS: usize = 100;
const MAX_FILES_SHOWN: usize = 40;
/// A change this big ought to say what it does.
const BIG_FILES: usize = 8;
const BIG_LINES: u64 = 300;
const SHOW_BUDGET: usize = 20_000;
const FIELD: char = '\u{1f}';
const RECORD: char = '\u{1e}';
const LOG_FORMAT: &str = "--format=%H%x1f%P%x1f%an%x1f%ae%x1f%aI%x1f%B%x1e";

#[derive(Clone, Debug)]
struct Info {
    hash: String,
    parents: Vec<String>,
    author: String,
    email: String,
    date: String,
    message: String,
}

impl Info {
    fn subject(&self) -> &str {
        self.message.lines().next().unwrap_or("").trim()
    }

    fn body(&self) -> String {
        self.message
            .lines()
            .skip(1)
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_string()
    }
}

fn parse(out: &str) -> Vec<Info> {
    out.split(RECORD)
        .filter_map(|record| {
            let record = record.trim_start_matches('\n');
            let mut fields = record.splitn(6, FIELD);
            let hash = fields.next()?.trim().to_string();
            if hash.is_empty() {
                return None;
            }
            let parents = fields
                .next()?
                .split_whitespace()
                .map(str::to_string)
                .collect();
            Some(Info {
                hash,
                parents,
                author: fields.next()?.to_string(),
                email: fields.next()?.to_string(),
                date: fields.next()?.to_string(),
                message: fields.next()?.trim_end().to_string(),
            })
        })
        .collect()
}

fn open(root: &Path) -> Result<Repo, String> {
    match Repo::open(root, Console::default()) {
        Ok(Some(repo)) => Ok(repo),
        Ok(None) => Err("This project is not a git repository".into()),
        Err(e) => Err(e.to_string()),
    }
}

/// The commits a request names, newest first: `last 10 commits`, `10`, `since abc123`, `abc..HEAD`.
fn select(repo: &Repo, range: &str) -> Result<Vec<Info>, String> {
    let text = range.trim();
    static NUMBER: OnceLock<Regex> = OnceLock::new();
    let number = NUMBER.get_or_init(|| Regex::new(r"\d+").expect("a valid expression"));
    let log = |args: &[&str]| -> Result<Vec<Info>, String> {
        let mut full = vec!["log", LOG_FORMAT];
        full.extend_from_slice(args);
        repo.read(&full)
            .map(|out| parse(&out))
            .map_err(|e| e.to_string())
    };
    let commits = if let Some((from, to)) = text.split_once("..") {
        let to = if to.trim().is_empty() {
            "HEAD"
        } else {
            to.trim()
        };
        let head = repo
            .read(&["rev-parse", "HEAD"])
            .map_err(|e| e.to_string())?
            .trim()
            .to_string();
        let end = repo
            .read(&["rev-parse", "--verify", &format!("{to}^{{commit}}")])
            .map_err(|e| format!("“{to}”: {e}"))?
            .trim()
            .to_string();
        if end != head {
            return Err("Only the latest commits of the current branch can be reworked: the range must end at HEAD".into());
        }
        log(&[
            &format!("{}..HEAD", from.trim()),
            "-n",
            &MAX_COMMITS.to_string(),
        ])?
    } else if let Some(rev) = text
        .to_lowercase()
        .strip_prefix("since ")
        .map(str::trim)
        .filter(|r| !r.is_empty())
    {
        log(&[&format!("{rev}..HEAD"), "-n", &MAX_COMMITS.to_string()])?
    } else if let Some(found) = number.find(text) {
        let n: usize = found
            .as_str()
            .parse()
            .map_err(|_| "that is not a number of commits".to_string())?;
        if n == 0 {
            return Err("Say how many commits to look at: at least 1".into());
        }
        log(&["-n", &n.min(MAX_COMMITS).to_string(), "HEAD"])?
    } else {
        return Err("Say which commits: a number (“last 10 commits”), “since <commit>”, or a range (“abc123..HEAD”)".into());
    };
    if commits.is_empty() {
        return Err("No commits in that range".into());
    }
    Ok(commits)
}

/// The hashes of the commits that are on no remote.
fn unpushed(repo: &Repo) -> HashSet<String> {
    repo.read(&["rev-list", "HEAD", "--not", "--remotes"])
        .unwrap_or_default()
        .lines()
        .map(|l| l.trim().to_string())
        .collect()
}

/// `(path, status, added, deleted)` of what a commit changes.
fn changes_of(repo: &Repo, hash: &str) -> Vec<(String, String, u64, u64)> {
    let numstat = repo
        .read(&["show", "--numstat", "--no-renames", "--format=", hash])
        .unwrap_or_default();
    let names = repo
        .read(&["show", "--name-status", "--no-renames", "--format=", hash])
        .unwrap_or_default();
    let status: std::collections::HashMap<&str, &str> = names
        .lines()
        .filter_map(|l| l.split_once('\t'))
        .map(|(s, p)| (p, s))
        .collect();
    numstat
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(3, '\t');
            let (added, deleted, path) = (parts.next()?, parts.next()?, parts.next()?);
            let kind = match status.get(path).copied().unwrap_or("M") {
                "A" => "added",
                "D" => "deleted",
                _ => "modified",
            };
            Some((
                path.to_string(),
                kind.to_string(),
                added.parse().unwrap_or(0),
                deleted.parse().unwrap_or(0),
            ))
        })
        .collect()
}

fn looks_like_a_test(path: &str) -> bool {
    let lower = path.to_lowercase();
    lower.contains("/test")
        || lower.starts_with("test")
        || lower.contains("spec")
        || lower.contains("__tests__")
        || lower.ends_with("tests.rs")
        || lower.contains("_test.")
}

/// Everything that does not match in a commit message, given what the commit changes.
fn findings(c: &Convention, message: &str, changes: &[(String, String, u64, u64)]) -> Vec<String> {
    let mut out = Vec::new();
    let subject = message.lines().next().unwrap_or("").trim();
    let body = message.lines().skip(1).collect::<Vec<_>>().join("\n");
    if let Err(reason) = commits::check(c, subject) {
        out.push(format!("subject: {reason}"));
    }
    if commits::clean_message(message) != message.trim() {
        out.push("contains a Co-authored-by line or an AI attribution (always removed)".into());
    }
    let paths: Vec<&str> = changes.iter().map(|c| c.0.as_str()).collect();
    if let Some(found) = Regex::new(r"^(?P<type>[A-Za-z]+)(?:\((?P<scope>[^)]+)\))?!?:")
        .ok()
        .and_then(|re| re.captures(subject))
    {
        let kind = found["type"].to_lowercase();
        let outside_docs: Vec<&&str> = paths
            .iter()
            .filter(|p| !commits::is_documentation(p))
            .collect();
        if kind == "docs" && !outside_docs.is_empty() {
            out.push(format!(
                "the type is docs but the commit also changes {}",
                outside_docs
                    .iter()
                    .take(3)
                    .map(|p| p.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if kind != "docs" && !paths.is_empty() && outside_docs.is_empty() {
            out.push("only documentation changes: the type should be docs".into());
        }
        if kind == "test" && !paths.iter().any(|p| looks_like_a_test(p)) {
            out.push("the type is test but no test file changes".into());
        }
        if kind == "ci"
            && paths.iter().any(|p| {
                !p.to_lowercase().starts_with(".github/") && !p.to_lowercase().contains("ci")
            })
        {
            out.push("the type is ci but the commit also changes other files".into());
        }
        if let Some(scope) = found.name("scope") {
            let lower: Vec<String> = paths.iter().map(|p| p.to_lowercase()).collect();
            let words: Vec<String> = scope
                .as_str()
                .to_lowercase()
                .split(|ch: char| !ch.is_alphanumeric())
                .filter(|w| w.len() > 2)
                .map(str::to_string)
                .collect();
            if !words.is_empty()
                && !words
                    .iter()
                    .any(|w| lower.iter().any(|p| p.contains(w.as_str())))
            {
                out.push(format!(
                    "the scope “{}” matches none of the changed paths",
                    scope.as_str()
                ));
            }
        }
    }
    // Files the message talks about that the commit does not touch.
    static FILES: OnceLock<Regex> = OnceLock::new();
    let files = FILES.get_or_init(|| {
        Regex::new(r"[\w./-]+\.(?:rs|jsx?|tsx?|md|json|toml|ya?ml|css|html|py|go|java|svg|lock)\b")
            .expect("a valid expression")
    });
    let mut missing: Vec<String> = Vec::new();
    for mention in files.find_iter(&format!("{subject}\n{body}")) {
        let name = mention.as_str().trim_start_matches("./");
        let file_name = name.rsplit('/').next().unwrap_or(name);
        if !paths
            .iter()
            .any(|p| p.ends_with(name) || p.rsplit('/').next() == Some(file_name))
            && !missing.iter().any(|m| m == name)
        {
            missing.push(name.to_string());
        }
    }
    if !missing.is_empty() {
        out.push(format!(
            "the message names {} which this commit does not change",
            missing.join(", ")
        ));
    }
    let lines: u64 = changes.iter().map(|c| c.2 + c.3).sum();
    if body.trim().is_empty()
        && (changes.len() >= BIG_FILES || lines >= BIG_LINES)
        && c.body_rate >= 0.3
    {
        out.push(format!(
            "a change of {} files and {lines} lines with no description, where bodies are common",
            changes.len()
        ));
    }
    out
}

/// `git_rework_plan`: the commits to look at, with what is wrong in each.
pub fn plan(root: &Path, args: &Value) -> Result<String, String> {
    let repo = open(root)?;
    let selected = select(&repo, args["range"].as_str().unwrap_or(""))?;
    let convention = commits::detect(&repo);
    let local = unpushed(&repo);
    let mut commits_json = Vec::new();
    let mut flagged = 0;
    for info in selected.iter().rev() {
        let changes = changes_of(&repo, &info.hash);
        let found = findings(&convention, &info.message, &changes);
        flagged += usize::from(!found.is_empty());
        let merge = info.parents.len() > 1;
        let pushed = !local.contains(&info.hash);
        commits_json.push(json!({
            "hash": info.hash,
            "short": info.hash.chars().take(7).collect::<String>(),
            "subject": info.subject(),
            "body": info.body(),
            "author": format!("{} <{}>", info.author, info.email),
            "date": info.date,
            "merge": merge,
            "pushed": pushed,
            "canRewrite": !merge,
            "files": changes.len(),
            "added": changes.iter().map(|c| c.2).sum::<u64>(),
            "deleted": changes.iter().map(|c| c.3).sum::<u64>(),
            "changes": changes.iter().take(MAX_FILES_SHOWN).map(|c| json!({"path": c.0, "status": c.1, "added": c.2, "deleted": c.3})).collect::<Vec<_>>(),
            "findings": found,
        }));
    }
    serde_json::to_string_pretty(&json!({
        "commits": commits_json,
        "flagged": flagged,
        "conventions": {"style": match convention.style { Style::Conventional => "conventional", Style::Gitmoji => "gitmoji", Style::Free => "free" }, "basedOnCommits": convention.samples},
        "guidance": commits::guidance(&convention),
        "next": "The commits are listed oldest first. `findings` are automatic checks, not a verdict: read each commit with git_show and judge whether (1) the subject follows `guidance`, (2) it says what the diff really does, (3) the body, if any, matches the diff and claims nothing that is not there. Keep the messages that are right. Then call git_rework_apply with only the commits that need a new message. Commits with pushed=true are on a remote: rewriting them changes shared history, so ask the user first and pass allowPushed only if they agree.",
    }))
    .map_err(|e| e.to_string())
}

/// `git_show`: one commit with its patch, to judge its message against.
pub fn show(root: &Path, args: &Value) -> Result<String, String> {
    let repo = open(root)?;
    let rev = args["rev"]
        .as_str()
        .filter(|r| !r.trim().is_empty())
        .ok_or("“rev” is required")?;
    if rev.starts_with('-') {
        return Err("“rev” is a commit, not an option".into());
    }
    let budget = args["maxChars"]
        .as_u64()
        .map_or(SHOW_BUDGET, |n| n as usize)
        .clamp(1_000, 100_000);
    let out = repo
        .read(&[
            "show",
            "--no-color",
            "--stat",
            "--patch",
            "--format=%h %s%n%n%b",
            rev,
        ])
        .map_err(|e| e.to_string())?;
    if out.len() <= budget {
        return Ok(out);
    }
    let mut cut = budget;
    while !out.is_char_boundary(cut) {
        cut -= 1;
    }
    Ok(format!(
        "{}\n… diff cut: raise maxChars to read more.\n",
        &out[..cut]
    ))
}

fn unix_time() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// `git_rework_apply`: rewrites the messages of the chosen commits, and nothing else.
pub fn apply(root: &Path, args: &Value) -> Result<String, String> {
    let repo = open(root)?;
    if repo.state() != crate::plugins::builtins::git::repo::State::Clean {
        return Err(
            "A merge, rebase, cherry-pick or revert is in progress: finish it first".into(),
        );
    }
    // The tree must be clean (new files do not matter): the head moves and the files stay as they are.
    let (head_info, changes) = repo.status().map_err(|e| e.to_string())?;
    if changes
        .iter()
        .any(|c| c.kind != crate::plugins::builtins::git::repo::Kind::Untracked)
    {
        return Err("The working tree has uncommitted changes: commit or stash them first".into());
    }
    let asked: Vec<(String, String)> = args["commits"]
        .as_array()
        .filter(|a| !a.is_empty())
        .ok_or("“commits” is required: a list of {hash, message}")?
        .iter()
        .map(|c| {
            Ok((
                c["hash"]
                    .as_str()
                    .ok_or("each commit needs a “hash”")?
                    .to_string(),
                c["message"]
                    .as_str()
                    .ok_or("each commit needs a “message”")?
                    .to_string(),
            ))
        })
        .collect::<Result<_, String>>()?;
    let allow_pushed = args["allowPushed"].as_bool().unwrap_or(false);

    let head = repo
        .read(&["rev-parse", "HEAD"])
        .map_err(|e| e.to_string())?
        .trim()
        .to_string();
    // Hash by hash: the commit, and the message to give it.
    let convention = commits::detect(&repo);
    let mut new_messages: Vec<(String, String)> = Vec::new();
    for (hash, message) in &asked {
        if hash.starts_with('-') {
            return Err(format!("“{hash}” is not a commit"));
        }
        let full = repo
            .read(&["rev-parse", "--verify", &format!("{hash}^{{commit}}")])
            .map_err(|e| format!("“{hash}”: {e}"))?
            .trim()
            .to_string();
        let message = commits::clean_message(message);
        commits::check(&convention, &message)
            .map_err(|e| format!("{}: {e}", &full[..7.min(full.len())]))?;
        if new_messages.iter().any(|(h, _)| *h == full) {
            return Err(format!("{} is listed twice", &full[..7.min(full.len())]));
        }
        new_messages.push((full, message));
    }
    // From the oldest commit to rewrite up to HEAD: all of them are replayed, in a straight line.
    let ancestry = repo
        .read(&[
            "rev-list",
            "--parents",
            "HEAD",
            "-n",
            &(MAX_COMMITS * 4).to_string(),
        ])
        .map_err(|e| e.to_string())?;
    let line: Vec<(String, Vec<String>)> = ancestry
        .lines()
        .map(|l| {
            let mut parts = l.split_whitespace().map(str::to_string);
            (parts.next().unwrap_or_default(), parts.collect())
        })
        .collect();
    let deepest = new_messages
        .iter()
        .map(|(h, _)| {
            line.iter().position(|(c, _)| c == h).ok_or_else(|| {
                format!(
                    "{} is not one of the latest commits of this branch",
                    &h[..7.min(h.len())]
                )
            })
        })
        .collect::<Result<Vec<usize>, String>>()?
        .into_iter()
        .max()
        .unwrap_or(0);
    if deepest + 1 > MAX_COMMITS {
        return Err(format!(
            "Only the latest {MAX_COMMITS} commits can be reworked at once"
        ));
    }
    let segment: Vec<&(String, Vec<String>)> = line[..=deepest].iter().rev().collect();
    if let Some((merge, _)) = segment.iter().find(|(_, parents)| parents.len() > 1) {
        return Err(format!(
            "{} is a merge: commits after a merge cannot be reworked this way",
            &merge[..7.min(merge.len())]
        ));
    }
    // Commits on a remote are shared: only with the user's agreement.
    let local = unpushed(&repo);
    let shared: Vec<&str> = new_messages
        .iter()
        .filter(|(h, _)| !local.contains(h))
        .map(|(h, _)| &h[..7.min(h.len())])
        .collect();
    if !shared.is_empty() && !allow_pushed {
        return Err(format!(
            "{} already on a remote: rewriting them changes shared history and needs a force push. Ask the user; pass allowPushed only if they agree.",
            shared.join(", ")
        ));
    }

    // The old head stays reachable under a ref until the user deletes it.
    let backup = format!("refs/atelier/backup/{}", unix_time());
    repo.read(&["update-ref", &backup, &head])
        .map_err(|e| e.to_string())?;
    let mut parent: Option<String> = segment
        .first()
        .and_then(|(_, parents)| parents.first().cloned());
    let mut mapping: Vec<(String, String)> = Vec::new();
    for (hash, _) in &segment {
        let found = repo
            .read(&["log", "-1", LOG_FORMAT, hash])
            .map_err(|e| e.to_string())?;
        let info = parse(&found)
            .into_iter()
            .next()
            .ok_or("cannot read a commit")?;
        let tree = repo
            .read(&["rev-parse", &format!("{hash}^{{tree}}")])
            .map_err(|e| e.to_string())?
            .trim()
            .to_string();
        let committer = repo
            .read(&["log", "-1", "--format=%cn%x1f%ce%x1f%cI", hash])
            .map_err(|e| e.to_string())?;
        let mut c = committer.trim_end().split(FIELD);
        let (cn, ce, cd) = (
            c.next().unwrap_or("").to_string(),
            c.next().unwrap_or("").to_string(),
            c.next().unwrap_or("").to_string(),
        );
        let message = match new_messages.iter().find(|(h, _)| h == hash) {
            Some((_, message)) => format!("{message}\n"),
            None => format!("{}\n", info.message),
        };
        let mut args: Vec<&str> = vec!["commit-tree", &tree];
        if let Some(p) = &parent {
            args.extend(["-p", p]);
        }
        args.extend(["-F", "-"]);
        let env = [
            ("GIT_AUTHOR_NAME", info.author.as_str()),
            ("GIT_AUTHOR_EMAIL", info.email.as_str()),
            ("GIT_AUTHOR_DATE", info.date.as_str()),
            ("GIT_COMMITTER_NAME", cn.as_str()),
            ("GIT_COMMITTER_EMAIL", ce.as_str()),
            ("GIT_COMMITTER_DATE", cd.as_str()),
        ];
        let made = repo
            .run_with_env(&args, &env, Some(message.into_bytes()))
            .map_err(|e| {
                format!(
                    "could not rewrite {}: {e}. Nothing moved; the old head is {backup}.",
                    &hash[..7.min(hash.len())]
                )
            })?;
        let made = made.trim().to_string();
        mapping.push((hash.clone(), made.clone()));
        parent = Some(made);
    }
    let new_head = parent.ok_or("nothing to rewrite")?;
    // The branch (or HEAD, when detached) moves only if nobody committed meanwhile.
    let target = match &head_info.branch {
        Some(branch) => format!("refs/heads/{branch}"),
        None => "HEAD".to_string(),
    };
    repo.read(&[
        "update-ref",
        "-m",
        "atelier: rework commit messages",
        &target,
        &new_head,
        &head,
    ])
    .map_err(|e| format!("the branch moved while reworking, nothing was changed: {e}"))?;
    let changed: Vec<String> = new_messages
        .iter()
        .filter_map(|(old, message)| {
            mapping.iter().find(|(o, _)| o == old).map(|(_, new)| {
                format!(
                    "- {} → {}: {}",
                    &old[..7.min(old.len())],
                    &new[..7.min(new.len())],
                    message.lines().next().unwrap_or("")
                )
            })
        })
        .collect();
    Ok(format!(
        "{} commit message(s) rewritten; {} commit(s) replayed on top, trees, authors and dates unchanged.\n{}\nThe previous head is kept as {backup}: `git reset --hard {backup}` puts everything back.{}",
        new_messages.len(),
        segment.len(),
        changed.join("\n"),
        if shared.is_empty() {
            ""
        } else {
            "\nSome of these commits were on a remote: pushing the branch now needs `git push --force-with-lease`, which Atelier does not do."
        }
    ))
}

/// The tools of this module, for the server. None for another tool.
pub fn tool(name: &str, args: &Value, root: &Path) -> Option<Result<String, String>> {
    Some(match name {
        "git_rework_plan" => plan(root, args),
        "git_show" => show(root, args),
        "git_rework_apply" => apply(root, args),
        _ => return None,
    })
}

/// The `rework-commit` command (an MCP prompt).
pub fn prompts() -> Vec<Value> {
    vec![json!({
        "name": "rework-commit",
        "title": "Rework commits",
        "description": "Review the messages of the latest commits (title and description against what each commit really changes) and rewrite the ones that do not match.",
        "arguments": [{"name": "range", "description": "Which commits: “last 10 commits”, “since abc123”, or “abc123..HEAD”. Asked if missing.", "required": false}],
    })]
}

pub fn prompt(name: &str, args: &Value) -> Option<String> {
    if name != "rework-commit" {
        return None;
    }
    let range = args["range"]
        .as_str()
        .map(str::trim)
        .filter(|r| !r.is_empty());
    let start = match range {
        Some(range) => format!("Rework the commits “{range}” of this project: check that every commit message matches what the commit does, and fix those that do not."),
        None => "Rework commit messages. First ask the user which commits (for example “last 10 commits”, “since abc123”).".to_string(),
    };
    Some(format!(
        "{start}\n\n\
         1. Call the Atelier tool `git_rework_plan` with the range. It lists the commits oldest first with their subject, body and the files they change, the repository's commit conventions, and automatic `findings`.\n\
         2. For each commit, check three things against its real changes (read them with `git_show`, or your own tools): the subject follows the repository's conventions (`guidance`); the subject says what the diff really does; the description (body), if any, matches the diff and claims nothing that is not there. `findings` are hints, not verdicts.\n\
         3. Keep the messages that are right. Call `git_rework_apply` with only the commits that need a new message ({{hash, message}}). Atelier checks every message against the conventions, rewrites only the messages (files, authors and dates stay) and keeps the old head under a backup ref.\n\n\
         Rules: never add a `Co-authored-by` line or any attribution to an AI (they are removed); do not rewrite commits already pushed to a remote unless the user explicitly agrees (then pass `allowPushed`); never push; at the end, tell the user which commits changed and how to go back."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{path::PathBuf, process::Command};

    fn git(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .current_dir(dir)
            .args(args)
            .output()
            .expect("git is installed");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    /// A repository where each commit writes `files` and has `message`.
    fn repo(name: &str, history: &[(&str, &[(&str, &str)])]) -> PathBuf {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("atelier-rework-{name}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        git(&dir, &["init", "-q"]);
        git(&dir, &["config", "user.email", "moi@example.com"]);
        git(&dir, &["config", "user.name", "Moi"]);
        git(&dir, &["config", "commit.gpgsign", "false"]);
        for (message, files) in history {
            for (path, content) in *files {
                let full = dir.join(path);
                std::fs::create_dir_all(full.parent().unwrap()).unwrap();
                std::fs::write(full, content).unwrap();
            }
            git(&dir, &["add", "-A"]);
            git(&dir, &["commit", "-q", "-m", message]);
        }
        dir
    }

    fn log(dir: &Path) -> Vec<String> {
        git(dir, &["log", "--format=%s"])
            .lines()
            .map(str::to_string)
            .collect()
    }

    const HISTORY: [(&str, &[(&str, &str)]); 5] = [
        ("feat(editor): ajoute le repli", &[("src/editor.rs", "a")]),
        (
            "fix(terminal): corrige le split",
            &[("src/terminal.rs", "b")],
        ),
        ("docs: met à jour le guide", &[("docs/guide.md", "c")]),
        ("Ajoute du code", &[("src/graph.rs", "d")]),
        ("docs: explique src/ui.rs", &[("src/other.rs", "e")]),
    ];

    #[test]
    fn the_range_can_be_said_in_several_ways() {
        let dir = repo("range", &HISTORY);
        let repo = open(&dir).unwrap();
        for (text, expected) in [
            ("last 3 commits", 3),
            ("2", 2),
            ("les 4 derniers commits", 4),
            ("since HEAD~2", 2),
            ("HEAD~3..HEAD", 3),
            ("last 99", 5),
        ] {
            assert_eq!(select(&repo, text).unwrap().len(), expected, "{text}");
        }
        assert!(select(&repo, "the recent ones").is_err());
        assert!(select(&repo, "0").is_err());
        assert!(
            select(&repo, "HEAD~3..HEAD~1")
                .unwrap_err()
                .contains("end at HEAD")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_plan_flags_what_does_not_match() {
        let dir = repo("plan", &HISTORY);
        let plan: Value =
            serde_json::from_str(&plan(&dir, &json!({"range": "last 5 commits"})).unwrap())
                .unwrap();
        let commits = plan["commits"].as_array().unwrap();
        assert_eq!(commits.len(), 5);
        assert_eq!(
            commits[0]["subject"], "feat(editor): ajoute le repli",
            "oldest first"
        );
        let findings = |i: usize| {
            commits[i]["findings"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|f| f.as_str())
                .collect::<Vec<_>>()
                .join(" | ")
        };
        assert_eq!(
            findings(0),
            "",
            "a good commit has nothing to say: {}",
            findings(0)
        );
        assert!(
            findings(2).contains("only documentation") || findings(2).is_empty(),
            "docs on a doc file is fine: {}",
            findings(2)
        );
        assert!(
            findings(3).contains("subject:"),
            "no type prefix where the history has one: {}",
            findings(3)
        );
        assert!(
            findings(4).contains("the type is docs but the commit also changes src/other.rs"),
            "{}",
            findings(4)
        );
        assert!(
            findings(4).contains("names src/ui.rs which this commit does not change"),
            "{}",
            findings(4)
        );
        assert_eq!(plan["flagged"], 2);
        assert_eq!(
            commits[0]["pushed"], false,
            "no remote at all: every commit is local"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn findings_cover_scopes_co_authors_and_big_undescribed_changes() {
        let c = commits::detect_from(&[
            "feat(a): x".into(),
            "fix(b): y".into(),
            "docs: z\n\nUn corps.".into(),
            "chore: w\n\nUn autre.".into(),
        ]);
        let one = vec![("src/graph/mod.rs".to_string(), "modified".to_string(), 3, 1)];
        assert!(
            findings(&c, "feat(terminal): add graph", &one)
                .iter()
                .any(|f| f.contains("scope “terminal”"))
        );
        assert!(findings(&c, "feat(graph): add graph", &one).is_empty());
        assert!(
            findings(
                &c,
                "feat(graph): add graph\n\nCo-Authored-By: Claude <x@y.z>",
                &one
            )
            .iter()
            .any(|f| f.contains("Co-authored-by"))
        );
        let many: Vec<_> = (0..10)
            .map(|i| (format!("src/f{i}.rs"), "modified".to_string(), 50, 0))
            .collect();
        assert!(
            findings(&c, "feat: lots", &many)
                .iter()
                .any(|f| f.contains("no description"))
        );
        assert!(findings(&c, "feat: lots\n\nWhat and why.", &many).is_empty());
        assert!(
            findings(&c, "test: cover it", &one)
                .iter()
                .any(|f| f.contains("no test file"))
        );
    }

    #[test]
    fn only_the_messages_change_and_the_old_head_is_kept() {
        let dir = repo("apply", &HISTORY);
        let before = git(&dir, &["log", "--format=%H|%T|%an|%aI"]);
        let hashes: Vec<String> = git(&dir, &["log", "--format=%H"])
            .lines()
            .map(str::to_string)
            .collect();
        let (bad_docs, bad_subject) = (&hashes[0], &hashes[1]);
        let done = apply(&dir, &json!({"commits": [
            {"hash": bad_docs, "message": "feat(graph): ajoute src/other.rs\n\nCo-Authored-By: Claude <noreply@anthropic.com>"},
            {"hash": bad_subject, "message": "feat(graph): ajoute le graphe"},
        ], "allowPushed": true}))
        .unwrap();
        assert!(
            done.contains("2 commit(s) replayed") || done.contains("2 commit message(s) rewritten"),
            "{done}"
        );
        let after = log(&dir);
        assert_eq!(after[0], "feat(graph): ajoute src/other.rs", "{after:?}");
        assert_eq!(after[1], "feat(graph): ajoute le graphe");
        assert_eq!(
            after[2], "docs: met à jour le guide",
            "older commits keep their message"
        );
        assert!(
            !git(&dir, &["log", "-1", "--format=%B"])
                .to_lowercase()
                .contains("co-authored"),
            "never a co-author"
        );
        // Trees, authors and dates are the same as before, commit by commit.
        let now = git(&dir, &["log", "--format=%T|%an|%aI"]);
        let was: Vec<String> = before
            .lines()
            .map(|l| l.splitn(2, '|').nth(1).unwrap().to_string())
            .collect();
        assert_eq!(now.lines().map(str::to_string).collect::<Vec<_>>(), was);
        // The commits below the first change keep their hash; the old head is a ref.
        assert_eq!(git(&dir, &["rev-parse", "HEAD~2"]).trim(), hashes[2]);
        let backups = git(&dir, &["for-each-ref", "refs/atelier/backup"]);
        assert!(
            backups.contains(&hashes[0]),
            "the old head is kept: {backups}"
        );
        assert_eq!(
            git(&dir, &["status", "--porcelain"]).trim(),
            "",
            "the files did not move"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn what_could_lose_work_is_refused_before_anything_moves() {
        let dir = repo("refuse", &HISTORY);
        let hashes: Vec<String> = git(&dir, &["log", "--format=%H"])
            .lines()
            .map(str::to_string)
            .collect();
        let head_before = git(&dir, &["rev-parse", "HEAD"]);
        let ok_message = json!([{"hash": hashes[0], "message": "feat(graph): ajoute x"}]);
        // A message that breaks the conventions, a hash of nowhere, an option instead of a hash.
        for bad in [
            json!([{"hash": hashes[0], "message": "Ajoute x"}]),
            json!([{"hash": "deadbeefdeadbeef", "message": "feat: x"}]),
            json!([{"hash": "--all", "message": "feat: x"}]),
            json!([]),
        ] {
            assert!(apply(&dir, &json!({"commits": bad, "allowPushed": true})).is_err());
        }
        // Uncommitted changes to tracked files: the head must not move under them.
        std::fs::write(dir.join("src/editor.rs"), "modified").unwrap();
        assert!(
            apply(&dir, &json!({"commits": ok_message, "allowPushed": true}))
                .unwrap_err()
                .contains("uncommitted")
        );
        git(&dir, &["checkout", "--", "src/editor.rs"]);
        assert_eq!(
            git(&dir, &["rev-parse", "HEAD"]),
            head_before,
            "nothing moved"
        );
        // A commit on a remote needs the user's word.
        let remote =
            std::env::temp_dir().join(format!("atelier-rework-remote-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&remote);
        git(
            &std::env::temp_dir(),
            &["init", "-q", "--bare", remote.to_str().unwrap()],
        );
        git(&dir, &["remote", "add", "origin", remote.to_str().unwrap()]);
        git(&dir, &["push", "-q", "origin", "HEAD:refs/heads/main"]);
        git(&dir, &["fetch", "-q", "origin"]);
        let plan: Value =
            serde_json::from_str(&plan(&dir, &json!({"range": "2"})).unwrap()).unwrap();
        assert_eq!(plan["commits"][0]["pushed"], true);
        let refused = apply(&dir, &json!({"commits": ok_message})).unwrap_err();
        assert!(
            refused.contains("already on a remote") && refused.contains("allowPushed"),
            "{refused}"
        );
        assert_eq!(git(&dir, &["rev-parse", "HEAD"]), head_before);
        assert!(
            apply(&dir, &json!({"commits": ok_message, "allowPushed": true}))
                .unwrap()
                .contains("force-with-lease")
        );
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&remote);
    }

    #[test]
    fn a_merge_in_the_way_is_refused() {
        let dir = repo(
            "merge",
            &[
                ("feat: un", &[("a.txt", "1")]),
                ("feat: deux", &[("b.txt", "2")]),
            ],
        );
        let main = git(&dir, &["branch", "--show-current"]).trim().to_string();
        git(&dir, &["checkout", "-q", "-b", "side", "HEAD~1"]);
        std::fs::write(dir.join("c.txt"), "3").unwrap();
        git(&dir, &["add", "-A"]);
        git(&dir, &["commit", "-q", "-m", "feat: trois"]);
        git(&dir, &["merge", "-q", "--no-ff", "-m", "Merge deux", &main]);
        let oldest = git(&dir, &["rev-list", "--max-parents=0", "HEAD"])
            .trim()
            .to_string();
        let refused = apply(&dir, &json!({"commits": [{"hash": oldest, "message": "feat: un, reformulé"}], "allowPushed": true})).unwrap_err();
        assert!(refused.contains("merge"), "{refused}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_command_is_offered_with_its_range() {
        let names: Vec<String> = prompts()
            .iter()
            .filter_map(|p| p["name"].as_str().map(str::to_string))
            .collect();
        assert_eq!(names, ["rework-commit"]);
        let text = prompt("rework-commit", &json!({"range": "last 10 commits"})).unwrap();
        assert!(
            text.contains("“last 10 commits”")
                && text.contains("git_rework_plan")
                && text.contains("git_rework_apply")
                && text.contains("Co-authored-by"),
            "{text}"
        );
        assert!(
            prompt("rework-commit", &json!({}))
                .unwrap()
                .contains("ask the user which commits")
        );
        assert_eq!(prompt("commit", &json!({})), None);
    }
}
