//! Pull requests through MCP: the `pr` command. The AI asks Atelier what the branch brings
//! (`git_pr_plan`): its commits against the base branch, each checked against the commit rules of
//! the repository, and its diff. Commits that break the rules are fixed first (`rework-commit`).
//! Then the AI writes the title and the description from what the commits really do, and Atelier
//! opens the pull request (`git_pull_request`): it checks every commit and the title again,
//! removes any attribution (`Co-authored-by`, "Generated with…") from the description, pushes the
//! branch and creates the pull request with `gh`.

use super::{
    commits::{self, Style},
    rework,
};
use crate::plugins::builtins::git::repo::{Console, Repo};
use regex::Regex;
use serde_json::{Value, json};
use std::{path::Path, process::Command, sync::OnceLock};

/// How much of the diff of the branch the plan shows.
const DIFF_BUDGET: usize = 30_000;
/// The most commits a pull request is read with.
const MAX_COMMITS: usize = 100;

fn open(root: &Path) -> Result<Repo, String> {
    match Repo::open(root, Console::default()) {
        Ok(Some(repo)) => Ok(repo),
        Ok(None) => Err("This project is not a git repository".into()),
        Err(e) => Err(e.to_string()),
    }
}

fn read(repo: &Repo, args: &[&str]) -> Result<String, String> {
    repo.read(args).map_err(|e| e.to_string())
}

/// The branch checked out, refused when there is none or when it is the base itself.
fn branch(repo: &Repo, base: &str) -> Result<String, String> {
    let branch = read(repo, &["rev-parse", "--abbrev-ref", "HEAD"])?
        .trim()
        .to_string();
    if branch == "HEAD" {
        return Err("No branch is checked out (detached HEAD): a pull request needs one".into());
    }
    if branch == base {
        return Err(format!(
            "The branch checked out is the base, {base}: create a branch for the changes first"
        ));
    }
    Ok(branch)
}

/// The branch a pull request goes into: the one asked, else the default branch of `origin`,
/// else `main`.
fn base_of(repo: &Repo, args: &Value) -> String {
    if let Some(base) = args["base"]
        .as_str()
        .map(str::trim)
        .filter(|b| !b.is_empty())
    {
        return base.to_string();
    }
    read(
        repo,
        &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
    )
    .ok()
    .and_then(|r| r.trim().strip_prefix("origin/").map(str::to_string))
    .filter(|b| !b.is_empty())
    .unwrap_or_else(|| "main".into())
}

/// The revision the base is compared from: `origin/<base>` when it exists, else `<base>`.
fn base_rev(repo: &Repo, base: &str) -> Result<String, String> {
    for rev in [format!("origin/{base}"), base.to_string()] {
        if read(
            repo,
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                &format!("{rev}^{{commit}}"),
            ],
        )
        .is_ok()
        {
            return Ok(rev);
        }
    }
    Err(format!("The base branch “{base}” does not exist"))
}

/// The commits of the branch, oldest first, each with what breaks the rules.
fn commits_of(
    repo: &Repo,
    base: &str,
    convention: &commits::Convention,
) -> Result<Vec<(rework::Info, Vec<String>)>, String> {
    let log = read(
        repo,
        &[
            "log",
            rework::LOG_FORMAT,
            &format!("{base}..HEAD"),
            "-n",
            &MAX_COMMITS.to_string(),
        ],
    )?;
    let mut found: Vec<(rework::Info, Vec<String>)> = rework::parse(&log)
        .into_iter()
        .map(|info| {
            let changes = rework::changes_of(repo, &info.hash);
            let mut problems = rework::findings(convention, &info.message, &changes);
            // Only what breaks the rules blocks a pull request: the other findings are hints.
            problems.retain(|p| p.starts_with("subject:") || p.contains("Co-authored-by"));
            (info, problems)
        })
        .collect();
    found.reverse();
    Ok(found)
}

/// Removes what never goes in a pull request: co-author lines, attributions to an AI or to the
/// tool that wrote it ("Generated with Claude Code", "🤖 …").
pub fn clean_description(text: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let attribution = RE.get_or_init(|| {
        Regex::new(r"(?i)^\s*(co-authored-by\s*:|.*\bgenerated\s+(with|by)\b|🤖)")
            .expect("a valid expression")
    });
    let text = text.replace("\r\n", "\n");
    let kept: Vec<&str> = text.lines().filter(|l| !attribution.is_match(l)).collect();
    kept.join("\n").trim().to_string()
}

/// `git_pr_plan`: what the branch brings, and whether its commits follow the rules.
pub fn plan(root: &Path, args: &Value) -> Result<String, String> {
    let repo = open(root)?;
    let base = base_of(&repo, args);
    let branch = branch(&repo, &base)?;
    let from = base_rev(&repo, &base)?;
    let convention = commits::detect(&repo);
    let commits = commits_of(&repo, &from, &convention)?;
    if commits.is_empty() {
        return Err(format!(
            "The branch {branch} has no commit that {base} does not have: nothing to propose"
        ));
    }
    let broken = commits.iter().filter(|(_, p)| !p.is_empty()).count();
    let stat = read(&repo, &["diff", "--stat", &format!("{from}...HEAD")])?;
    let mut diff = read(&repo, &["diff", &format!("{from}...HEAD")])?;
    let truncated = diff.len() > DIFF_BUDGET;
    if truncated {
        let mut end = DIFF_BUDGET;
        while !diff.is_char_boundary(end) {
            end -= 1;
        }
        diff.truncate(end);
    }
    let dirty = !read(&repo, &["status", "--porcelain"])?.trim().is_empty();
    serde_json::to_string_pretty(&json!({
        "branch": branch,
        "base": base,
        "commits": commits.iter().map(|(info, problems)| json!({
            "short": info.hash.chars().take(7).collect::<String>(),
            "subject": info.subject(),
            "body": info.body(),
            "problems": problems,
        })).collect::<Vec<_>>(),
        "commitsBreakingTheRules": broken,
        "uncommittedChanges": dirty,
        "stat": stat.trim(),
        "diff": diff,
        "diffTruncated": truncated,
        "conventions": {"style": match convention.style { Style::Conventional => "conventional", Style::Gitmoji => "gitmoji", Style::Free => "free" }},
        "guidance": format!(
            "Title: one line that says what the whole branch does, written as a commit subject of this repository (with a single commit, its subject).\n{}\n\nDescription, in the language of the commits, in Markdown: what the branch changes and why, then the changes grouped by subject (a short list), then how it was tested if the commits or the diff say so. Describe only what the diff and the commits really do: no guess, no promise, no list of files for its own sake. No title line in the description. Never add a Co-authored-by line, \"Generated with…\" or any attribution to an AI or a tool: they are removed.",
            commits::guidance(&convention)
        ),
        "next": if broken > 0 {
            "Some commits break the rules of the repository: the pull request is refused until they are fixed. Tell the user, and fix them with the rework-commit flow (git_rework_plan, git_rework_apply) if they agree, then call git_pr_plan again."
        } else {
            "Read the commits and the diff (more with git_show), write the title and the description following `guidance`, then call git_pull_request."
        },
    }))
    .map_err(|e| e.to_string())
}

/// `git_pull_request`: checks the commits and the title, pushes the branch, opens the pull
/// request.
pub fn create(root: &Path, args: &Value) -> Result<String, String> {
    let repo = open(root)?;
    let base = base_of(&repo, args);
    let branch = branch(&repo, &base)?;
    let from = base_rev(&repo, &base)?;
    let convention = commits::detect(&repo);
    let commits = commits_of(&repo, &from, &convention)?;
    if commits.is_empty() {
        return Err(format!(
            "The branch {branch} has no commit that {base} does not have"
        ));
    }
    let broken: Vec<String> = commits
        .iter()
        .filter(|(_, p)| !p.is_empty())
        .map(|(info, p)| {
            format!(
                "{} {}: {}",
                info.hash.chars().take(7).collect::<String>(),
                info.subject(),
                p.join("; ")
            )
        })
        .collect();
    if !broken.is_empty() {
        return Err(format!(
            "These commits break the rules of the repository; fix them first (rework-commit):\n{}",
            broken.join("\n")
        ));
    }
    let title = args["title"].as_str().unwrap_or("").trim();
    if title.lines().count() != 1 {
        return Err("The title is one line".into());
    }
    commits::check(&convention, title).map_err(|e| format!("title: {e}"))?;
    let body = clean_description(args["body"].as_str().unwrap_or(""));
    if body.is_empty() {
        return Err("The description is empty: say what the branch does".into());
    }
    // The branch on the remote, then the pull request.
    read(&repo, &["push", "--set-upstream", "origin", &branch])
        .map_err(|e| format!("git push: {e}"))?;
    let file = std::env::temp_dir().join(format!("atelier-pr-{}.md", std::process::id()));
    std::fs::write(&file, &body).map_err(|e| e.to_string())?;
    let mut gh = Command::new("gh");
    gh.current_dir(&repo.root).args([
        "pr",
        "create",
        "--base",
        &base,
        "--head",
        &branch,
        "--title",
        title,
        "--body-file",
    ]);
    gh.arg(&file);
    if args["draft"].as_bool() == Some(true) {
        gh.arg("--draft");
    }
    let output = gh.output();
    let _ = std::fs::remove_file(&file);
    let output = output.map_err(|e| {
        format!("gh, the GitHub command line, is needed to open a pull request: {e}")
    })?;
    if !output.status.success() {
        return Err(format!(
            "gh pr create: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(format!(
        "Pull request opened: {}\n{} commit(s) of {branch} into {base}, all following the rules of the repository.",
        String::from_utf8_lossy(&output.stdout).trim(),
        commits.len()
    ))
}

/// The tools of this module, for the server. None for another tool.
pub fn tool(name: &str, args: &Value, root: &Path) -> Option<Result<String, String>> {
    Some(match name {
        "git_pr_plan" => plan(root, args),
        "git_pull_request" => create(root, args),
        _ => return None,
    })
}

/// The `pr` command (an MCP prompt).
pub fn prompts() -> Vec<Value> {
    vec![json!({
        "name": "pr",
        "title": "Pull request",
        "description": "Open a pull request of the current branch: its commits checked against the rules of the repository, a title and an exact description of what it does, with no attribution.",
        "arguments": [{"name": "base", "description": "The branch to merge into (by default the default branch of origin).", "required": false}],
    })]
}

pub fn prompt(name: &str, args: &Value) -> Option<String> {
    if name != "pr" {
        return None;
    }
    let base = args["base"]
        .as_str()
        .map(str::trim)
        .filter(|b| !b.is_empty());
    let into = base.map_or(String::new(), |b| format!(" into “{b}”"));
    let with_base = base.map_or(String::new(), |b| format!(" with base “{b}”"));
    Some(format!(
        "Open a pull request of the current branch{into}.\n\n\
         1. Call the Atelier tool `git_pr_plan`{with_base}. It returns the commits of the branch against the base, each checked against the commit rules of the repository (`problems`), the diff, and the `guidance` for the title and the description.\n\
         2. If some commits break the rules, tell the user which and why, and fix them with the rework-commit flow (`git_rework_plan`, `git_rework_apply`) once they agree; then call `git_pr_plan` again. If there are uncommitted changes, ask the user whether to commit them first (the commit flow).\n\
         3. Read the commits and the diff (`git_show` for more), then write the title and the description following `guidance` exactly: what the branch really does, nothing it does not.\n\
         4. Call `git_pull_request` with the title and the description{with_base}. Atelier checks the commits and the title again, removes any attribution, pushes the branch and opens the pull request.\n\n\
         Rules: never add a `Co-authored-by` line, \"Generated with…\" or any attribution to an AI or a tool, in the title, the description or a commit; never force-push; give the user the address of the pull request."
    ))
}

#[cfg(test)]
#[path = "../../../../test/plugins/builtins/mcp/pr/tests.rs"]
mod tests;
