//! The pull request of the current branch, its reviews and the comments of its reviews, read with
//! the GitHub CLI (`gh`), which has the account of the user.

use serde_json::Value;
use std::{path::Path, process::Command};

#[derive(Clone, Debug, PartialEq)]
pub struct Review {
    pub author: String,
    /// APPROVED, CHANGES_REQUESTED, COMMENTED, DISMISSED or PENDING.
    pub state: String,
    pub body: String,
    pub date: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Comment {
    pub author: String,
    pub body: String,
    pub date: String,
}

/// A conversation on a line of a file: the first comment and its replies.
#[derive(Clone, Debug, PartialEq)]
pub struct Thread {
    pub path: String,
    /// 1-based, on the current code; None when the code it was about changed since.
    pub line: Option<usize>,
    pub comments: Vec<Comment>,
    pub url: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Pull {
    pub number: u64,
    pub title: String,
    pub url: String,
    /// OPEN, CLOSED or MERGED.
    pub state: String,
    pub draft: bool,
    /// APPROVED, CHANGES_REQUESTED, REVIEW_REQUIRED, or empty.
    pub decision: String,
    pub author: String,
    pub head: String,
    pub base: String,
    pub reviews: Vec<Review>,
    pub threads: Vec<Thread>,
}

fn gh(root: &Path, args: &[&str]) -> Result<String, String> {
    let mut command = Command::new("gh");
    command.args(args).current_dir(root);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let output = command
        .output()
        .map_err(|_| tr!("github.gh_not_found").to_string())?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

fn text(value: &Value, pointer: &str) -> String {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// The pull request of the branch checked out in `root`; None when it has none.
pub fn fetch(root: &Path) -> Result<Option<Pull>, String> {
    let fields =
        "number,title,url,state,isDraft,reviewDecision,author,headRefName,baseRefName,reviews";
    let view = match gh(root, &["pr", "view", "--json", fields]) {
        Ok(view) => view,
        Err(error) if error.contains("no pull requests found") => return Ok(None),
        Err(error) => return Err(error),
    };
    let view: Value = serde_json::from_str(&view).map_err(|e| e.to_string())?;
    let mut pull = read_pull(&view);
    // `https://github.com/<owner>/<repo>/pull/<number>`.
    let repo: Vec<&str> = pull.url.split('/').skip(3).take(2).collect();
    if let [owner, name] = repo.as_slice() {
        let endpoint = format!(
            "repos/{owner}/{name}/pulls/{}/comments?per_page=100",
            pull.number
        );
        let comments = gh(root, &["api", "--paginate", &endpoint])?;
        pull.threads = threads(&comments);
    }
    Ok(Some(pull))
}

pub fn read_pull(view: &Value) -> Pull {
    let reviews = view
        .get("reviews")
        .and_then(Value::as_array)
        .map(|reviews| {
            reviews
                .iter()
                .map(|r| Review {
                    author: text(r, "/author/login"),
                    state: text(r, "/state"),
                    body: text(r, "/body"),
                    date: text(r, "/submittedAt"),
                })
                .collect()
        })
        .unwrap_or_default();
    Pull {
        number: view
            .get("number")
            .and_then(Value::as_u64)
            .unwrap_or_default(),
        title: text(view, "/title"),
        url: text(view, "/url"),
        state: text(view, "/state"),
        draft: view
            .get("isDraft")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        decision: text(view, "/reviewDecision"),
        author: text(view, "/author/login"),
        head: text(view, "/headRefName"),
        base: text(view, "/baseRefName"),
        reviews,
        threads: Vec::new(),
    }
}

/// The comments of the reviews, by conversation, in the order of the files and lines. `gh api
/// --paginate` writes one JSON array per page.
pub fn threads(pages: &str) -> Vec<Thread> {
    let comments: Vec<Value> = serde_json::Deserializer::from_str(pages)
        .into_iter::<Vec<Value>>()
        .filter_map(Result::ok)
        .flatten()
        .collect();
    let mut threads: Vec<Thread> = Vec::new();
    // The conversation of each comment, by its id: a reply may answer a reply.
    let mut of: std::collections::HashMap<u64, usize> = std::collections::HashMap::new();
    for c in &comments {
        let comment = Comment {
            author: text(c, "/user/login"),
            body: text(c, "/body"),
            date: text(c, "/created_at"),
        };
        let id = c.get("id").and_then(Value::as_u64).unwrap_or_default();
        let replied = c.get("in_reply_to_id").and_then(Value::as_u64);
        let index = match replied.and_then(|r| of.get(&r).copied()) {
            Some(index) => {
                threads[index].comments.push(comment);
                index
            }
            None => {
                threads.push(Thread {
                    path: text(c, "/path"),
                    line: c.get("line").and_then(Value::as_u64).map(|l| l as usize),
                    comments: vec![comment],
                    url: text(c, "/html_url"),
                });
                threads.len() - 1
            }
        };
        of.insert(id, index);
    }
    threads.sort_by(|a, b| (&a.path, a.line).cmp(&(&b.path, b.line)));
    threads
}
