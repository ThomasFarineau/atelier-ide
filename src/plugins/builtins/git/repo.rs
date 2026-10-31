//! Git backend: runs the `git` executable found in the PATH and parses its machine-readable output.
//! Paths inside the repository are relative and use `/`, as git prints them.

use anyhow::{Result, anyhow, bail};
use std::{
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Mutex},
};

pub struct ConsoleEntry {
    pub command: String,
    pub output: String,
    pub ok: bool,
}

/// Commands run on behalf of the user and their output, shown in the Git console.
pub type Console = Arc<Mutex<Vec<ConsoleEntry>>>;

#[derive(Clone)]
pub struct Repo {
    /// Canonical work tree root: absolute paths built from it match the project tree.
    pub root: PathBuf,
    /// The root without the Windows `\\?\` prefix, used as git's working directory.
    dir: PathBuf,
    git_dir: PathBuf,
    pub console: Console,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Modified,
    Added,
    Deleted,
    Renamed,
    Untracked,
    Conflicted,
}

#[derive(Clone, Debug)]
pub struct Change {
    pub path: String,
    /// Previous path of a rename.
    pub orig: Option<String>,
    pub kind: Kind,
}

#[derive(Clone, Default, Debug)]
pub struct Head {
    /// None when detached.
    pub branch: Option<String>,
    /// None before the first commit.
    pub oid: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
pub enum State {
    #[default]
    Clean,
    Merging,
    Rebasing,
    CherryPicking,
    Reverting,
}

#[derive(Clone, Debug)]
pub struct Branch {
    /// `main` for a local branch, `origin/main` for a remote one.
    pub name: String,
    pub remote: bool,
    pub current: bool,
    pub upstream: Option<String>,
    /// `>` ahead, `<` behind, `<>` diverged, `=` in sync, empty without upstream.
    pub track: String,
}

#[derive(Clone, Debug)]
pub struct Stash {
    /// `stash@{0}`
    pub name: String,
    pub message: String,
}

#[derive(Clone, Debug)]
pub struct Remote {
    pub name: String,
    pub url: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RefKind {
    /// Detached HEAD.
    Head,
    Local,
    Remote,
    Tag,
}

#[derive(Clone, Debug)]
pub struct Ref {
    pub name: String,
    pub kind: RefKind,
    /// The branch HEAD points to.
    pub current: bool,
}

#[derive(Clone, Debug)]
pub struct Commit {
    pub hash: String,
    pub parents: Vec<String>,
    pub author: String,
    pub email: String,
    pub date: String,
    pub refs: Vec<Ref>,
    pub subject: String,
}

#[derive(Clone, Default, PartialEq, Eq)]
pub struct LogQuery {
    /// None: all branches, remotes and tags.
    pub branch: Option<String>,
    /// History of one file, following renames; the log has no graph then.
    pub path: Option<String>,
    pub limit: usize,
}

#[derive(Clone, Default, Debug)]
pub struct BlameLine {
    pub hash: String,
    pub author: String,
    pub time: i64,
    pub tz: String,
    pub summary: String,
}

/// Removes the `\\?\` prefix of canonical Windows paths, which git does not understand.
pub fn simplify(path: &Path) -> PathBuf {
    crate::project::simplify(path)
}

/// Environment of every git command.
const GIT_ENV: [(&str, &str); 4] = [
    // Never wait for a password on a terminal nobody sees; credential helpers still work.
    ("GIT_TERMINAL_PROMPT", "0"),
    // Continue a rebase, a merge or a cherry-pick without opening an editor.
    ("GIT_EDITOR", "true"),
    ("GIT_MERGE_AUTOEDIT", "no"),
    // File names are never glob patterns.
    ("GIT_LITERAL_PATHSPECS", "1"),
];

/// A git command in `dir`. A folder of a WSL distribution is handled by the git of that
/// distribution, as VS Code does, not by Windows git through the network share.
fn command(dir: &Path, extra_env: &[(&str, &str)]) -> Command {
    let mut cmd;
    if let Some(wsl) = crate::wsl::parse(dir) {
        cmd = Command::new("wsl.exe");
        cmd.args(["-d", &wsl.distro, "--cd", &wsl.linux, "--exec", "env"]);
        for (key, value) in GIT_ENV.iter().chain(extra_env) {
            cmd.arg(format!("{key}={value}"));
        }
        cmd.arg("git").current_dir(std::env::temp_dir());
    } else {
        cmd = Command::new("git");
        cmd.current_dir(dir);
        for (key, value) in GIT_ENV.iter().chain(extra_env) {
            cmd.env(key, value);
        }
    }
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

struct Output {
    stdout: String,
    stderr: String,
    ok: bool,
}

fn execute(mut cmd: Command, input: Option<Vec<u8>>) -> Result<Output> {
    if input.is_some() {
        cmd.stdin(Stdio::piped());
    }
    let mut child = cmd.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            anyhow!(tr!("git.git_cannot_found_install_add"))
        } else {
            anyhow!(trf!("git.cannot_start_git", e = e))
        }
    })?;
    if let (Some(input), Some(mut stdin)) = (input, child.stdin.take()) {
        // A separate thread, so a large input cannot block while git fills its output pipes.
        std::thread::spawn(move || stdin.write_all(&input));
    }
    let output = child.wait_with_output()?;
    Ok(Output {
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        ok: output.status.success(),
    })
}

fn failure(output: &Output) -> anyhow::Error {
    let message = [output.stderr.trim(), output.stdout.trim()]
        .into_iter()
        .find(|m| !m.is_empty())
        .unwrap_or(tr!("git.git_failed"));
    anyhow!("{}", message.trim_start_matches("fatal: "))
}

fn display(args: &[&str]) -> String {
    std::iter::once("git")
        .chain(args.iter().copied())
        .map(|a| {
            if a.is_empty() || a.contains(' ') {
                format!("\"{a}\"")
            } else {
                a.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

impl Repo {
    /// Ok(None) when `path` is not inside a work tree; Err when git cannot run.
    pub fn open(path: &Path, console: Console) -> Result<Option<Self>> {
        let mut cmd = command(&simplify(path), &[]);
        cmd.args(["rev-parse", "--show-toplevel", "--absolute-git-dir"]);
        let output = execute(cmd, None)?;
        if !output.ok {
            return Ok(None);
        }
        let mut lines = output.stdout.lines();
        let (Some(top), Some(git_dir)) = (lines.next(), lines.next()) else {
            return Ok(None);
        };
        // The git of a WSL distribution prints Linux paths: back to the Windows share.
        let (root, git_dir) = match crate::wsl::parse(&simplify(path)) {
            Some(wsl) => (
                crate::wsl::to_windows(&wsl.distro, top),
                crate::wsl::to_windows(&wsl.distro, git_dir),
            ),
            None => (PathBuf::from(top), PathBuf::from(git_dir)),
        };
        let root = root.canonicalize().unwrap_or(root);
        Ok(Some(Self {
            dir: simplify(&root),
            root,
            git_dir,
            console,
        }))
    }

    pub fn init(path: &Path, console: &Console) -> Result<()> {
        Self::logged(path, &["init"], None, console).map(drop)
    }

    pub fn clone_into(url: &str, parent: &Path, name: &str, console: &Console) -> Result<String> {
        Self::logged(
            parent,
            &["clone", "--progress", "--", url, name],
            None,
            console,
        )
    }

    fn logged(
        dir: &Path,
        args: &[&str],
        input: Option<Vec<u8>>,
        console: &Console,
    ) -> Result<String> {
        let mut cmd = command(&simplify(dir), &[]);
        cmd.args(args);
        let output = execute(cmd, input);
        let (text, ok) = match &output {
            Ok(o) => (
                format!("{}{}", o.stdout, o.stderr).trim_end().to_string(),
                o.ok,
            ),
            Err(e) => (e.to_string(), false),
        };
        if let Ok(mut console) = console.lock() {
            console.push(ConsoleEntry {
                command: display(args),
                output: text,
                ok,
            });
            let excess = console.len().saturating_sub(500);
            console.drain(..excess);
        }
        let output = output?;
        if !output.ok {
            return Err(failure(&output));
        }
        Ok(output.stdout)
    }

    /// Runs a command on behalf of the user: it appears in the Git console.
    pub fn run(&self, args: &[&str]) -> Result<String> {
        Self::logged(&self.dir, args, None, &self.console)
    }

    /// Like [`Repo::run`], with paths passed on stdin: no command line length limit.
    pub fn run_paths(&self, args: &[&str], paths: &[String]) -> Result<String> {
        let mut full = args.to_vec();
        full.extend(["--pathspec-from-file=-", "--pathspec-file-nul"]);
        let input = paths.join("\0").into_bytes();
        Self::logged(&self.dir, &full, Some(input), &self.console)
    }

    /// Runs a command with environment variables of its own (an author, a date) and an input,
    /// not shown in the console: for plumbing such as `commit-tree`.
    pub fn run_with_env(
        &self,
        args: &[&str],
        env: &[(&str, &str)],
        input: Option<Vec<u8>>,
    ) -> Result<String> {
        let mut cmd = command(&self.dir, env);
        cmd.args(args);
        let output = execute(cmd, input)?;
        if !output.ok {
            return Err(failure(&output));
        }
        Ok(output.stdout)
    }

    /// Runs a query, not shown in the console.
    pub fn read(&self, args: &[&str]) -> Result<String> {
        // Background refreshes must not take locks the user's own git commands need.
        let mut cmd = command(&self.dir, &[("GIT_OPTIONAL_LOCKS", "0")]);
        cmd.args(args);
        let output = execute(cmd, None)?;
        if !output.ok {
            return Err(failure(&output));
        }
        Ok(output.stdout)
    }

    pub fn abs(&self, rel: &str) -> PathBuf {
        let mut path = self.root.clone();
        path.extend(rel.split('/').filter(|p| !p.is_empty()));
        path
    }

    pub fn rel(&self, path: &Path) -> Option<String> {
        let simple = simplify(path);
        let inner = path
            .strip_prefix(&self.root)
            .or_else(|_| simple.strip_prefix(&self.dir))
            .ok()?;
        let parts: Vec<_> = inner
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        Some(parts.join("/"))
    }

    pub fn status(&self) -> Result<(Head, Vec<Change>)> {
        let out = self.read(&[
            "status",
            "--porcelain=v2",
            "-z",
            "--branch",
            "--untracked-files=all",
        ])?;
        Ok(parse_status(&out))
    }

    /// Of `rels` (relative to the root), the ones git tracks, or that hold tracked files.
    pub fn tracked(&self, rels: &[String]) -> Vec<String> {
        // The paths are taken literally: `GIT_LITERAL_PATHSPECS` is set.
        let mut args = vec!["ls-files", "-z", "--"];
        args.extend(rels.iter().map(String::as_str));
        let out = self.read(&args).unwrap_or_default();
        let files: Vec<&str> = out.split('\0').filter(|p| !p.is_empty()).collect();
        rels.iter()
            .filter(|r| {
                files
                    .iter()
                    .any(|f| *f == r.as_str() || f.starts_with(&format!("{r}/")))
            })
            .cloned()
            .collect()
    }

    /// Ignored files and folders (a folder that is ignored as a whole counts once).
    pub fn ignored(&self) -> Vec<PathBuf> {
        let out = self
            .read(&[
                "ls-files",
                "-z",
                "--others",
                "--ignored",
                "--exclude-standard",
                "--directory",
            ])
            .unwrap_or_default();
        out.split('\0')
            .filter(|p| !p.is_empty())
            .map(|p| self.abs(p.trim_end_matches('/')))
            .collect()
    }

    pub fn state(&self) -> State {
        let has = |name: &str| self.git_dir.join(name).exists();
        if has("rebase-merge") || has("rebase-apply") {
            State::Rebasing
        } else if has("MERGE_HEAD") {
            State::Merging
        } else if has("CHERRY_PICK_HEAD") {
            State::CherryPicking
        } else if has("REVERT_HEAD") {
            State::Reverting
        } else {
            State::Clean
        }
    }

    /// Message prepared by git for a merge, cherry-pick or revert in progress.
    pub fn merge_message(&self) -> Option<String> {
        let text = std::fs::read_to_string(self.git_dir.join("MERGE_MSG")).ok()?;
        let message: Vec<_> = text.lines().filter(|l| !l.starts_with('#')).collect();
        Some(message.join("\n").trim().to_string()).filter(|m| !m.is_empty())
    }

    pub fn branches(&self) -> Result<Vec<Branch>> {
        let out = self.read(&[
            "for-each-ref",
            "--format=%(refname)%00%(HEAD)%00%(upstream:short)%00%(upstream:trackshort)",
            "refs/heads",
            "refs/remotes",
        ])?;
        Ok(parse_branches(&out))
    }

    pub fn stashes(&self) -> Result<Vec<Stash>> {
        let out = self.read(&["stash", "list", "--format=%gd%x00%gs"])?;
        Ok(out
            .lines()
            .filter_map(|l| l.split_once('\0'))
            .map(|(name, message)| Stash {
                name: name.into(),
                message: message.into(),
            })
            .collect())
    }

    pub fn remotes(&self) -> Result<Vec<Remote>> {
        let out = self.read(&["remote", "-v"])?;
        Ok(out
            .lines()
            .filter(|l| l.ends_with("(fetch)"))
            .filter_map(|l| {
                let (name, rest) = l.split_once('\t')?;
                Some(Remote {
                    name: name.into(),
                    url: rest.trim_end_matches("(fetch)").trim().into(),
                })
            })
            .collect())
    }

    /// Commits in date order, children always before their parents.
    pub fn log(&self, query: &LogQuery) -> Result<Vec<Commit>> {
        let limit = format!("-n{}", query.limit.max(1));
        let mut args = vec![
            "log",
            "--no-color",
            "--decorate=full",
            "--date=format-local:%d/%m/%Y %H:%M",
            &limit,
            "--format=%H%x1f%P%x1f%an%x1f%ae%x1f%ad%x1f%D%x1f%s",
        ];
        if query.path.is_none() {
            args.push("--date-order");
        }
        match &query.branch {
            Some(branch) => args.push(branch),
            None => args.extend(["HEAD", "--branches", "--remotes", "--tags"]),
        }
        if let Some(path) = &query.path {
            args.extend(["--follow", "--", path]);
        }
        Ok(parse_log(&self.read(&args)?))
    }

    pub fn message(&self, rev: &str) -> Result<String> {
        Ok(self
            .read(&["show", "-s", "--format=%B", rev])?
            .trim()
            .to_string())
    }

    /// Files changed between two revisions; `from` None means against the empty tree.
    pub fn changed_files(&self, from: Option<&str>, to: &str) -> Result<Vec<Change>> {
        let mut args = vec![
            "diff-tree",
            "-r",
            "-z",
            "-M",
            "--no-commit-id",
            "--name-status",
        ];
        match from {
            Some(from) => args.extend([from, to]),
            None => args.extend(["--root", to]),
        }
        Ok(parse_name_status(&self.read(&args)?))
    }

    /// Content of a file at a revision (`HEAD`, a hash, `stash@{0}`…), None if it does not exist.
    pub fn show(&self, rev: &str, path: &str) -> Option<String> {
        self.read(&["show", &format!("{rev}:{path}")]).ok()
    }

    pub fn blame(&self, path: &str) -> Result<Vec<BlameLine>> {
        Ok(parse_blame(&self.read(&[
            "blame",
            "--line-porcelain",
            "--",
            path,
        ])?))
    }

    /// One line per commit that would be pushed: `hash subject`.
    pub fn outgoing(&self, upstream: Option<&str>) -> Result<Vec<String>> {
        let range = upstream.map(|u| format!("{u}..HEAD"));
        let mut args = vec!["log", "--no-color", "--format=%h  %s", "-n200"];
        match &range {
            Some(range) => args.push(range),
            None => args.extend(["HEAD", "--not", "--remotes"]),
        }
        Ok(self.read(&args)?.lines().map(str::to_string).collect())
    }
}

fn kind_from_letter(letter: char) -> Kind {
    match letter {
        'A' | 'C' => Kind::Added,
        'D' => Kind::Deleted,
        'R' => Kind::Renamed,
        'U' => Kind::Conflicted,
        _ => Kind::Modified,
    }
}

pub fn parse_status(out: &str) -> (Head, Vec<Change>) {
    let mut head = Head::default();
    let mut changes = Vec::new();
    let mut fields = out.split('\0');
    while let Some(entry) = fields.next() {
        if let Some(header) = entry.strip_prefix("# ") {
            let (key, value) = header.split_once(' ').unwrap_or((header, ""));
            match key {
                "branch.oid" => head.oid = (value != "(initial)").then(|| value.into()),
                "branch.head" => head.branch = (value != "(detached)").then(|| value.into()),
                "branch.upstream" => head.upstream = Some(value.into()),
                "branch.ab" => {
                    for part in value.split(' ') {
                        if let Some(n) = part.strip_prefix('+') {
                            head.ahead = n.parse().unwrap_or(0);
                        } else if let Some(n) = part.strip_prefix('-') {
                            head.behind = n.parse().unwrap_or(0);
                        }
                    }
                }
                _ => {}
            }
            continue;
        }
        let change = match entry.chars().next() {
            Some('1') => entry.splitn(9, ' ').nth(8).map(|path| {
                let xy = &entry[2..4];
                let kind = ['A', 'D', 'R']
                    .into_iter()
                    .find(|c| xy.contains(*c))
                    .map_or(Kind::Modified, kind_from_letter);
                (path, None, kind)
            }),
            Some('2') => entry.splitn(10, ' ').nth(9).map(|path| {
                let kind = if entry[2..4].contains('C') {
                    Kind::Added
                } else {
                    Kind::Renamed
                };
                (path, fields.next(), kind)
            }),
            Some('u') => entry
                .splitn(11, ' ')
                .nth(10)
                .map(|path| (path, None, Kind::Conflicted)),
            Some('?') => entry.get(2..).map(|path| (path, None, Kind::Untracked)),
            _ => None,
        };
        if let Some((path, orig, kind)) = change {
            changes.push(Change {
                path: path.into(),
                orig: orig.map(Into::into),
                kind,
            });
        }
    }
    changes.sort_by(|a, b| a.path.cmp(&b.path));
    (head, changes)
}

pub fn parse_branches(out: &str) -> Vec<Branch> {
    out.lines()
        .filter_map(|line| {
            let mut parts = line.split('\0');
            let refname = parts.next()?;
            let current = parts.next()? == "*";
            let upstream = parts.next().filter(|u| !u.is_empty()).map(Into::into);
            let track = parts.next().unwrap_or("").to_string();
            let (name, remote) = if let Some(name) = refname.strip_prefix("refs/heads/") {
                (name, false)
            } else {
                (refname.strip_prefix("refs/remotes/")?, true)
            };
            if remote && (name.ends_with("/HEAD") || !name.contains('/')) {
                return None;
            }
            Some(Branch {
                name: name.into(),
                remote,
                current,
                upstream,
                track,
            })
        })
        .collect()
}

fn parse_refs(decoration: &str) -> Vec<Ref> {
    decoration
        .split(", ")
        .filter(|d| !d.is_empty())
        .filter_map(|d| {
            let (current, d) = match d.strip_prefix("HEAD -> ") {
                Some(rest) => (true, rest),
                None => (false, d),
            };
            let (name, kind) = if d == "HEAD" {
                ("HEAD", RefKind::Head)
            } else if let Some(tag) = d.strip_prefix("tag: refs/tags/") {
                (tag, RefKind::Tag)
            } else if let Some(local) = d.strip_prefix("refs/heads/") {
                (local, RefKind::Local)
            } else {
                // A remote branch; its `HEAD` is only a pointer to one of them.
                let remote = d.strip_prefix("refs/remotes/")?;
                if remote.ends_with("/HEAD") {
                    return None;
                }
                (remote, RefKind::Remote)
            };
            Some(Ref {
                name: name.into(),
                kind,
                current,
            })
        })
        .collect()
}

/// One commit per line, fields separated by `\x1f`.
pub fn parse_log(out: &str) -> Vec<Commit> {
    out.lines()
        .filter(|line| line.contains('\x1f'))
        .map(|line| {
            let f: Vec<&str> = line.splitn(7, '\x1f').collect();
            let field = |i: usize| f.get(i).copied().unwrap_or("").to_string();
            Commit {
                hash: field(0),
                parents: field(1).split_whitespace().map(Into::into).collect(),
                author: field(2),
                email: field(3),
                date: field(4),
                refs: parse_refs(f.get(5).copied().unwrap_or("")),
                subject: field(6),
            }
        })
        .collect()
}

pub fn parse_name_status(out: &str) -> Vec<Change> {
    let mut changes = Vec::new();
    let mut fields = out.split('\0').filter(|f| !f.is_empty());
    while let Some(status) = fields.next() {
        let letter = status.chars().next().unwrap_or('M');
        let kind = kind_from_letter(letter);
        let change = if matches!(letter, 'R' | 'C') {
            let orig = fields.next();
            fields.next().map(|path| Change {
                path: path.into(),
                orig: orig.map(Into::into),
                kind,
            })
        } else {
            fields.next().map(|path| Change {
                path: path.into(),
                orig: None,
                kind,
            })
        };
        changes.extend(change);
    }
    changes
}

pub fn parse_blame(out: &str) -> Vec<BlameLine> {
    let mut lines = Vec::new();
    let mut current = BlameLine::default();
    for line in out.split('\n') {
        if line.starts_with('\t') {
            lines.push(current.clone());
            continue;
        }
        let (key, value) = line.split_once(' ').unwrap_or((line, ""));
        let is_hash = key.len() >= 40 && key.bytes().all(|b| b.is_ascii_hexdigit());
        match key {
            _ if is_hash => current.hash = key.into(),
            "author" => current.author = value.into(),
            "author-time" => current.time = value.parse().unwrap_or(0),
            "author-tz" => current.tz = value.into(),
            "summary" => current.summary = value.into(),
            _ => {}
        }
    }
    lines
}

/// `YYYY-MM-DD` of a Unix time in the author's time zone (`+0200`).
pub fn date(time: i64, tz: &str) -> String {
    let sign = if tz.starts_with('-') { -1 } else { 1 };
    let digits = tz.trim_start_matches(['+', '-']);
    let hours: i64 = digits.get(..2).and_then(|h| h.parse().ok()).unwrap_or(0);
    let minutes: i64 = digits.get(2..4).and_then(|m| m.parse().ok()).unwrap_or(0);
    let days = (time + sign * (hours * 3600 + minutes * 60)).div_euclid(86_400);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

pub fn ensure_name(name: &str, what: &str) -> Result<()> {
    if name.trim().is_empty() || name.contains(char::is_whitespace) || name.starts_with('-') {
        bail!(trf!("git.invalid", what = what));
    }
    Ok(())
}

#[cfg(test)]
#[path = "../../../../test/plugins/builtins/git/repo/tests.rs"]
pub mod tests;
