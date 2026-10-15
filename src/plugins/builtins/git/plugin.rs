//! The Git built-in plugin: state, background work and the operations behind the views.

mod views;

use super::{
    conflict::{self, Segment},
    diff::{self, Row},
    graph::{self, GraphRow},
    repo::{
        self, Branch, Change, Commit, Console, Head, Kind, LogQuery, Remote, Repo, Stash, State,
    },
};
use crate::plugins::builtin::{BuiltinPlugin, Command, Dock, Host, LineMark, ToolWindow};
use crate::ui::{self, Theme};
use anyhow::{Result, bail};
use eframe::egui::{self, Color32};
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    path::{Path, PathBuf},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub static TOOL_WINDOWS: [ToolWindow; 2] = [
    ToolWindow {
        id: "git.commit",
        title: crate::i18n::Text("git.commit"),
        icon: "commit",
        dock: Dock::Left,
        shortcut: "Alt+0",
    },
    ToolWindow {
        id: "git.log",
        title: crate::i18n::Text("git.git"),
        icon: "git",
        dock: Dock::Bottom,
        shortcut: "Alt+9",
    },
];
const COMMIT_WINDOW: &ToolWindow = &TOOL_WINDOWS[0];
const LOG_WINDOW: &ToolWindow = &TOOL_WINDOWS[1];

const REFRESH_EVERY: Duration = Duration::from_secs(5);
const LOG_LIMIT: usize = 1500;
const SMART_STASH: &str = "Atelier : checkout intelligent";

/// Color of a file name in the change lists and the project tree, as in IntelliJ.
pub fn status_color(theme: &Theme, kind: Kind) -> Color32 {
    match kind {
        Kind::Modified | Kind::Renamed => theme.modified,
        Kind::Added => theme.added,
        Kind::Deleted => theme.deleted,
        Kind::Untracked => theme.unversioned,
        Kind::Conflicted => theme.conflict,
    }
}

#[derive(Clone, Default)]
struct Snapshot {
    head: Head,
    changes: Vec<Change>,
    branches: Vec<Branch>,
    stashes: Vec<Stash>,
    remotes: Vec<Remote>,
    state: State,
    merge_message: Option<String>,
    ignored: Vec<PathBuf>,
}

fn load(repo: &Repo) -> Result<Snapshot> {
    let (head, changes) = repo.status()?;
    Ok(Snapshot {
        head,
        changes,
        branches: repo.branches().unwrap_or_default(),
        stashes: repo.stashes().unwrap_or_default(),
        remotes: repo.remotes().unwrap_or_default(),
        state: repo.state(),
        merge_message: repo.merge_message(),
        ignored: repo.ignored(),
    })
}

type Task = Box<dyn FnOnce() -> Result<String> + Send>;

/// What to do once an operation succeeded.
enum After {
    Nothing,
    Committed { push: bool },
    Reopen,
    OpenProject(PathBuf),
    Message(String),
}

struct Job {
    label: String,
    after: After,
    handle: JoinHandle<Result<String>>,
}

/// An operation waiting for the user's confirmation.
struct Pending {
    label: String,
    after: After,
    task: Task,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ResetMode {
    Soft,
    Mixed,
    Hard,
    Keep,
}

#[derive(Clone)]
enum Target {
    Branch(String),
    Remote(String),
    Revision(String),
}

enum Dialog {
    Confirm {
        title: String,
        text: String,
        items: Vec<String>,
        pending: Pending,
    },
    NewBranch {
        from: Option<String>,
        name: String,
        checkout: bool,
    },
    RenameBranch {
        old: String,
        name: String,
    },
    DeleteBranch {
        name: String,
        force: bool,
    },
    NewTag {
        rev: String,
        name: String,
        message: String,
    },
    Revision {
        rev: String,
    },
    SmartCheckout {
        target: Target,
    },
    Push {
        remote: String,
        branch: String,
        target: String,
        force: bool,
        tags: bool,
        commits: Vec<String>,
        upstream: bool,
    },
    Update {
        rebase: bool,
        autostash: bool,
    },
    Stash {
        message: String,
        untracked: bool,
    },
    Reset {
        rev: String,
        subject: String,
        mode: ResetMode,
    },
    Clone {
        url: String,
        parent: String,
        name: String,
    },
    Remotes {
        name: String,
        url: String,
    },
}

#[derive(Clone)]
enum DiffSource {
    WorkingTree(Change),
    Revision {
        from: Option<String>,
        to: String,
        change: Change,
    },
}

struct DiffView {
    path: String,
    left_title: String,
    right_title: String,
    old: String,
    new: String,
    binary: bool,
    side_by_side: bool,
    collapse: bool,
    rows: Vec<Row>,
    starts: Vec<usize>,
    current: usize,
    scroll_to: Option<usize>,
}

impl DiffView {
    fn rebuild(&mut self) {
        let context = self.collapse.then_some(4);
        self.rows = if self.side_by_side {
            diff::side_by_side(&self.old, &self.new, context)
        } else {
            diff::unified(&self.old, &self.new, context)
        };
        self.starts = diff::change_starts(&self.rows);
        self.current = 0;
        self.scroll_to = self.starts.first().copied();
    }
    fn step(&mut self, forward: bool) {
        if self.starts.is_empty() {
            return;
        }
        let n = self.starts.len();
        self.current = if forward {
            (self.current + 1) % n
        } else {
            (self.current + n - 1) % n
        };
        self.scroll_to = Some(self.starts[self.current]);
    }
}

struct Resolver {
    path: String,
    segments: Vec<Segment>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CommitTab {
    Changes,
    Stash,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LogTab {
    Log,
    Console,
}

#[derive(Default)]
struct Log {
    query: LogQuery,
    rows: Vec<Commit>,
    /// Lanes of `rows`, empty for the history of a file.
    graph: Vec<GraphRow>,
    loading: Option<JoinHandle<Result<Vec<Commit>>>>,
    stale: bool,
    error: Option<String>,
    filter: String,
    /// Shows only the commits of this author (filtered locally).
    author: Option<String>,
    selected: Option<String>,
    /// Message and files of the selected commit.
    details: Option<(String, String, Vec<Change>)>,
}

pub struct GitPlugin {
    project: Option<PathBuf>,
    repo: Option<Repo>,
    error: Option<String>,
    console: Console,
    snap: Snapshot,
    loaded: bool,
    refresh: Option<JoinHandle<Result<Snapshot>>>,
    refreshed_at: Option<Instant>,
    stale: bool,
    focused: bool,
    job: Option<Job>,
    /// Balloons waiting for the next frame: (text, error).
    notices: Vec<(String, bool)>,
    theme: Theme,
    #[cfg(debug_assertions)]
    previewed: bool,
    /// Collapsed tree nodes (commit tool window, log branches, branch popup sections).
    collapsed: HashSet<String>,
    // Commit tool window.
    commit_tab: CommitTab,
    message: String,
    amend: bool,
    signoff: bool,
    excluded: BTreeSet<String>,
    included_new: BTreeSet<String>,
    selected: Option<String>,
    focus_message: bool,
    stash_selected: Option<String>,
    stash_files: Vec<Change>,
    // Git tool window.
    log_tab: LogTab,
    log: Log,
    // Windows.
    diff: Option<DiffView>,
    resolver: Option<Resolver>,
    dialog: Option<Dialog>,
    branch_filter: String,
    /// Branch popup: anchor, which corner of the popup sits on it, and the pass it opened in.
    branch_popup: Option<(egui::Pos2, egui::Align2, u64)>,
    /// Branch whose actions are shown next to the branch popup, with the row's top right.
    branch_sub: Option<(Branch, egui::Pos2)>,
    // Editor and project tree.
    colors: HashMap<PathBuf, Kind>,
    changed_dirs: HashSet<PathBuf>,
    /// Ignored files and folders; what lies inside an ignored folder is ignored too.
    ignored: HashSet<PathBuf>,
    bases: HashMap<PathBuf, Option<String>>,
    marks: HashMap<PathBuf, (String, Vec<LineMark>)>,
    annotated: BTreeSet<PathBuf>,
    blame: HashMap<PathBuf, String>,
    blame_jobs: HashMap<PathBuf, JoinHandle<Result<String>>>,
}

impl Default for GitPlugin {
    fn default() -> Self {
        Self {
            project: None,
            repo: None,
            error: None,
            console: Console::default(),
            snap: Snapshot::default(),
            loaded: false,
            refresh: None,
            refreshed_at: None,
            stale: true,
            focused: true,
            job: None,
            notices: Vec::new(),
            theme: ui::theme_by_index(0),
            #[cfg(debug_assertions)]
            previewed: false,
            collapsed: ["changes:git.unversioned_files".to_string()].into(),
            commit_tab: CommitTab::Changes,
            message: String::new(),
            amend: false,
            signoff: false,
            excluded: BTreeSet::new(),
            included_new: BTreeSet::new(),
            selected: None,
            focus_message: false,
            stash_selected: None,
            stash_files: Vec::new(),
            log_tab: LogTab::Log,
            log: Log {
                stale: true,
                query: LogQuery {
                    limit: LOG_LIMIT,
                    ..Default::default()
                },
                ..Default::default()
            },
            diff: None,
            resolver: None,
            dialog: None,
            branch_filter: String::new(),
            branch_popup: None,
            branch_sub: None,
            colors: HashMap::new(),
            changed_dirs: HashSet::new(),
            ignored: HashSet::new(),
            bases: HashMap::new(),
            marks: HashMap::new(),
            annotated: BTreeSet::new(),
            blame: HashMap::new(),
            blame_jobs: HashMap::new(),
        }
    }
}

fn first_line(text: &str) -> &str {
    text.lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim()
}

impl GitPlugin {
    fn open(&mut self, root: PathBuf) {
        let console = self.console.clone();
        let theme = self.theme;
        *self = Self {
            console,
            theme,
            ..Self::default()
        };
        match Repo::open(&root, self.console.clone()) {
            Ok(repo) => self.repo = repo,
            Err(e) => self.error = Some(format!("{e:#}")),
        }
        self.project = Some(root);
    }

    /// Queues a balloon notification, shown by the IDE on the next frame.
    fn toast(&mut self, message: impl Into<String>, error: bool) {
        self.notices.push((message.into(), error));
    }

    /// Forgets cached repository data after an operation or an outside change.
    fn invalidate(&mut self) {
        self.stale = true;
        self.log.stale = true;
        self.bases.clear();
        self.marks.clear();
        self.blame.clear();
    }

    /// True for an ignored file, or anything inside an ignored folder.
    fn is_ignored(&self, path: &Path) -> bool {
        !self.ignored.is_empty() && path.ancestors().any(|p| self.ignored.contains(p))
    }

    fn apply(&mut self, snap: Snapshot) {
        let head_moved = snap.head.oid != self.snap.head.oid
            || snap.head.branch != self.snap.head.branch
            || snap.branches.len() != self.snap.branches.len()
            || snap.stashes.len() != self.snap.stashes.len();
        if head_moved {
            self.invalidate();
            self.stale = false;
        }
        // A file that was not tracked may have been added since: look for its base again.
        self.bases.retain(|_, base| base.is_some());
        if snap.state != State::Clean && self.message.trim().is_empty() {
            self.message = snap.merge_message.clone().unwrap_or_default();
        }
        self.colors.clear();
        self.changed_dirs.clear();
        self.ignored = snap.ignored.iter().cloned().collect();
        if let Some(repo) = &self.repo {
            for change in &snap.changes {
                let path = repo.abs(&change.path);
                if change.kind != Kind::Untracked {
                    let mut dir = path.parent();
                    while let Some(d) = dir.filter(|d| d.starts_with(&repo.root)) {
                        if !self.changed_dirs.insert(d.to_path_buf()) {
                            break;
                        }
                        dir = d.parent();
                    }
                }
                self.colors.insert(path, change.kind);
            }
        }
        let paths: HashSet<_> = snap.changes.iter().map(|c| c.path.as_str()).collect();
        self.excluded.retain(|p| paths.contains(p.as_str()));
        self.included_new.retain(|p| paths.contains(p.as_str()));
        if self
            .selected
            .as_ref()
            .is_some_and(|s| !paths.contains(s.as_str()))
        {
            self.selected = None;
        }
        if self
            .stash_selected
            .as_ref()
            .is_some_and(|s| !snap.stashes.iter().any(|st| &st.name == s))
        {
            self.stash_selected = None;
            self.stash_files.clear();
        }
        self.snap = snap;
        self.loaded = true;
    }

    fn poll(&mut self, ctx: &egui::Context, host: &mut dyn Host) {
        self.theme = ui::theme(ctx);
        for (text, error) in std::mem::take(&mut self.notices) {
            host.balloon("Git", text, error);
        }
        let root = host.root().to_path_buf();
        if self.project.as_ref() != Some(&root) {
            self.open(root);
        }
        let focused = ctx.input(|i| i.viewport().focused.unwrap_or(true));
        if focused && !self.focused {
            if self.repo.is_none() {
                // `git init` may have been run in the terminal.
                self.project = None;
                return;
            }
            self.stale = true;
        }
        self.focused = focused;
        if self
            .refreshed_at
            .is_none_or(|t| t.elapsed() > REFRESH_EVERY)
        {
            self.stale = true;
        }
        if let Some(repo) = &self.repo
            && self.stale
            && self.refresh.is_none()
        {
            let repo = repo.clone();
            self.refresh = Some(thread::spawn(move || load(&repo)));
            self.stale = false;
            self.refreshed_at = Some(Instant::now());
        }
        if self.refresh.as_ref().is_some_and(JoinHandle::is_finished) {
            match self.refresh.take().map(JoinHandle::join) {
                Some(Ok(Ok(snap))) => self.apply(snap),
                Some(Ok(Err(e))) => self.error = Some(format!("{e:#}")),
                _ => {}
            }
        }
        #[cfg(debug_assertions)]
        if self.loaded && !self.previewed {
            self.previewed = true;
            if let Ok(what) = std::env::var("ATELIER_GIT_PREVIEW") {
                self.preview(ctx, &what, host);
            }
        }
        if self.job.as_ref().is_some_and(|j| j.handle.is_finished())
            && let Some(job) = self.job.take()
        {
            self.finish(job, host);
        }
        let finished: Vec<_> = self
            .blame_jobs
            .iter()
            .filter(|(_, h)| h.is_finished())
            .map(|(p, _)| p.clone())
            .collect();
        for path in finished {
            if let Some(handle) = self.blame_jobs.remove(&path) {
                match handle.join() {
                    Ok(Ok(text)) => {
                        self.blame.insert(path, text);
                    }
                    Ok(Err(e)) => {
                        self.annotated.remove(&path);
                        self.toast(trf!("git.cannot_annotate", e = format!("{:#}", e)), true);
                    }
                    Err(_) => {}
                }
            }
        }
        if self
            .log
            .loading
            .as_ref()
            .is_some_and(JoinHandle::is_finished)
            && let Some(Ok(result)) = self.log.loading.take().map(JoinHandle::join)
        {
            match result {
                Ok(rows) => {
                    self.log.graph = if self.log.query.path.is_none() {
                        graph::layout(rows.iter().map(|c| (c.hash.as_str(), c.parents.as_slice())))
                    } else {
                        Vec::new()
                    };
                    self.log.rows = rows;
                    self.log.error = None;
                }
                Err(e) => self.log.error = Some(format!("{e:#}")),
            }
        }
        let busy = self.refresh.is_some()
            || self.job.is_some()
            || self.log.loading.is_some()
            || !self.blame_jobs.is_empty();
        ctx.request_repaint_after(if busy {
            Duration::from_millis(100)
        } else {
            REFRESH_EVERY
        });
    }

    /// Debug builds only: opens views that need clicks, for screenshots taken without input.
    /// `ATELIER_GIT_PREVIEW` lists them, separated by commas: diff, commit, branches, push, update.
    #[cfg(debug_assertions)]
    fn preview(&mut self, ctx: &egui::Context, what: &str, host: &mut dyn Host) {
        for part in what.split(',') {
            match part.trim() {
                "diff" => {
                    let change = self
                        .snap
                        .changes
                        .iter()
                        .find(|c| c.kind == Kind::Modified && c.path.ends_with(".rs"))
                        .cloned();
                    if let Some(change) = change {
                        self.selected = Some(change.path.clone());
                        self.open_diff(DiffSource::WorkingTree(change));
                    }
                }
                "commit" => {
                    self.load_log();
                    if let Some(Ok(Ok(rows))) = self.log.loading.take().map(JoinHandle::join) {
                        self.log.graph = graph::layout(
                            rows.iter().map(|c| (c.hash.as_str(), c.parents.as_slice())),
                        );
                        self.log.rows = rows;
                    }
                    if let Some(hash) = self.log.rows.first().map(|c| c.hash.clone()) {
                        self.select_commit(hash);
                    }
                }
                "branches" => {
                    self.branch_popup = Some((
                        egui::pos2(150.0, 40.0),
                        egui::Align2::LEFT_TOP,
                        ctx.cumulative_pass_nr(),
                    ));
                    if let Some(branch) = self.snap.branches.iter().find(|b| b.current).cloned() {
                        self.branch_sub = Some((branch, egui::pos2(522.0, 300.0)));
                    }
                }
                "push" => self.open_push(host),
                "update" => self.open_update(),
                _ => {}
            }
        }
    }

    fn finish(&mut self, job: Job, host: &mut dyn Host) {
        let result = job
            .handle
            .join()
            .unwrap_or_else(|_| bail!(tr!("git.operation_crashed")));
        host.reload_files();
        self.invalidate();
        match result {
            Ok(_) => {
                host.balloon(&job.label, tr!("git.done").into(), false);
                match job.after {
                    After::Nothing => {}
                    After::Committed { push } => {
                        self.message.clear();
                        self.amend = false;
                        self.excluded.clear();
                        self.included_new.clear();
                        if push {
                            self.open_push(host);
                        }
                    }
                    After::Reopen => self.project = None,
                    After::OpenProject(path) => host.open_project(path),
                    After::Message(message) => self.message = message,
                }
            }
            Err(e) => {
                let text = format!("{e:#}");
                let shown: String = text.chars().take(600).collect();
                let shown = if shown.trim().is_empty() {
                    first_line(&text).to_string()
                } else {
                    shown
                };
                host.balloon(&trf!("git.failed", label = job.label), shown, true);
            }
        }
    }

    fn spawn_task(&mut self, label: impl Into<String>, after: After, task: Task) {
        if self.job.is_some() {
            self.toast(tr!("git.git_operation_already_running"), true);
            return;
        }
        self.job = Some(Job {
            label: label.into(),
            after,
            handle: thread::spawn(task),
        });
    }

    /// Runs an operation on the repository in the background.
    fn spawn(
        &mut self,
        label: impl Into<String>,
        after: After,
        task: impl FnOnce(&Repo) -> Result<String> + Send + 'static,
    ) {
        let Some(repo) = self.repo.clone() else {
            return;
        };
        self.spawn_task(label, after, Box::new(move || task(&repo)));
    }

    fn pending(
        &self,
        label: impl Into<String>,
        after: After,
        task: impl FnOnce(&Repo) -> Result<String> + Send + 'static,
    ) -> Option<Pending> {
        let repo = self.repo.clone()?;
        Some(Pending {
            label: label.into(),
            after,
            task: Box::new(move || task(&repo)),
        })
    }

    fn confirm(&mut self, title: &str, text: String, items: Vec<String>, pending: Option<Pending>) {
        if let Some(pending) = pending {
            self.dialog = Some(Dialog::Confirm {
                title: title.into(),
                text,
                items,
                pending,
            });
        }
    }

    /// Runs a simple git command line in the background.
    fn git(&mut self, label: &str, args: &[&str]) {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        self.spawn(label, After::Nothing, move |git| {
            git.run(&args.iter().map(String::as_str).collect::<Vec<_>>())
        });
    }

    fn included(&self, change: &Change) -> bool {
        if change.kind == Kind::Untracked {
            self.included_new.contains(&change.path)
        } else {
            !self.excluded.contains(&change.path)
        }
    }

    fn set_included(&mut self, change: &Change, on: bool) {
        let path = change.path.clone();
        match (change.kind == Kind::Untracked, on) {
            (true, true) => self.included_new.insert(path),
            (true, false) => self.included_new.remove(&path),
            (false, true) => self.excluded.remove(&path),
            (false, false) => self.excluded.insert(path),
        };
    }

    fn checked(&self) -> Vec<Change> {
        self.snap
            .changes
            .iter()
            .filter(|c| self.included(c))
            .cloned()
            .collect()
    }

    fn has_local_changes(&self) -> bool {
        self.snap.changes.iter().any(|c| c.kind != Kind::Untracked)
    }

    fn current_branch(&self) -> Option<&str> {
        self.snap.head.branch.as_deref()
    }

    // ---- Commit -------------------------------------------------------------------------

    fn commit(&mut self, host: &mut dyn Host, push: bool) {
        let message = self.message.trim().to_string();
        if message.is_empty() {
            self.toast(tr!("git.enter_commit_message"), true);
            return;
        }
        let changes = self.checked();
        if changes.iter().any(|c| c.kind == Kind::Conflicted) {
            self.toast(tr!("git.resolve_conflicts_before_committing"), true);
            return;
        }
        let whole_index = self.snap.state != State::Clean;
        if changes.is_empty() && !self.amend && !whole_index {
            self.toast(tr!("git.no_file_selected_commit"), true);
            return;
        }
        if !host.save_files() {
            return;
        }
        let add: Vec<String> = changes.iter().map(|c| c.path.clone()).collect();
        let mut paths = add.clone();
        paths.extend(changes.iter().filter_map(|c| c.orig.clone()));
        let (amend, signoff) = (self.amend, self.signoff);
        // Errors in the code to commit are said first; the commit goes on when confirmed.
        let files: Vec<PathBuf> = add.iter().map(|p| host.root().join(p)).collect();
        let problems: Vec<_> = host
            .inspect_files(&files)
            .into_iter()
            .filter(|p| p.severity == 1)
            .collect();
        let task = move |git: &Repo| -> Result<String> {
            if !add.is_empty() {
                git.run_paths(&["add", "-A"], &add)?;
            }
            let mut args = vec!["commit", "-m", &message];
            if amend {
                args.push("--amend");
            }
            if signoff {
                args.push("--signoff");
            }
            if whole_index {
                // A merge commit takes the whole index: partial commits are refused.
                git.run(&args)
            } else if paths.is_empty() {
                // Amend only the message, not what may already be staged.
                args.push("--only");
                git.run(&args)
            } else {
                git.run_paths(&args, &paths)
            }
        };
        if problems.is_empty() {
            self.spawn("Commit", After::Committed { push }, task);
            return;
        }
        let mut items: Vec<String> = Vec::new();
        for problem in &problems {
            let name = problem
                .path
                .strip_prefix(host.root())
                .unwrap_or(&problem.path)
                .display()
                .to_string();
            let line = format!("{name}:{}  {}", problem.line + 1, problem.message.lines().next().unwrap_or(""));
            if !items.contains(&line) && items.len() < 12 {
                items.push(line);
            }
        }
        let pending = self.pending("Commit", After::Committed { push }, task);
        self.confirm(
            tr!("git.errors_before_commit"),
            trf!("git.errors_in_files_commit_anyway", count = problems.len()),
            items,
            pending,
        );
    }

    fn toggle_amend(&mut self) {
        if self.amend
            && self.message.trim().is_empty()
            && let Some(repo) = &self.repo
        {
            self.message = repo.message("HEAD").unwrap_or_default();
        }
    }

    fn rollback(&mut self, changes: Vec<Change>) {
        if changes.is_empty() {
            self.toast(tr!("git.select_files_roll_back"), true);
            return;
        }
        let items = changes.iter().map(|c| c.path.clone()).collect();
        let mut unstage = Vec::new();
        let mut restore = Vec::new();
        let mut delete = Vec::new();
        for change in changes {
            match change.kind {
                Kind::Added => unstage.push(change.path),
                Kind::Untracked => delete.push(change.path),
                _ => {
                    restore.extend(change.orig);
                    restore.push(change.path);
                }
            }
        }
        let text = if delete.is_empty() {
            tr!("git.local_changes_these_files_will").into()
        } else {
            tr!("git.local_changes_will_lost_unversioned").into()
        };
        let pending = self.pending(tr!("git.rolling_back"), After::Nothing, move |git| {
            if !unstage.is_empty() {
                git.run_paths(&["reset", "-q"], &unstage)?;
            }
            if !restore.is_empty() {
                git.run_paths(
                    &["restore", "--source=HEAD", "--staged", "--worktree"],
                    &restore,
                )?;
            }
            for path in delete {
                std::fs::remove_file(git.abs(&path))?;
            }
            Ok(String::new())
        });
        self.confirm(tr!("git.rollback_changes"), text, items, pending);
    }

    fn add_to_vcs(&mut self, paths: Vec<String>) {
        self.spawn(tr!("git.adding_git"), After::Nothing, move |git| {
            git.run_paths(&["add"], &paths)
        });
    }

    fn ignore(&mut self, path: String) {
        self.spawn(tr!("git.adding_gitignore"), After::Nothing, move |git| {
            let file = git.abs(".gitignore");
            let mut text = std::fs::read_to_string(&file).unwrap_or_default();
            if !text.is_empty() && !text.ends_with('\n') {
                text.push('\n');
            }
            text.push_str(&format!("/{path}\n"));
            std::fs::write(file, text)?;
            Ok(String::new())
        });
    }

    fn accept(&mut self, path: String, theirs: bool) {
        let side = if theirs { "--theirs" } else { "--ours" };
        self.spawn(tr!("git.resolving_conflict"), After::Nothing, move |git| {
            git.run(&["checkout", side, "--", &path])?;
            git.run(&["add", "--", &path])
        });
    }

    fn mark_resolved(&mut self, path: String) {
        self.spawn(tr!("git.conflict_resolved"), After::Nothing, move |git| {
            git.run(&["add", "--", &path])
        });
    }

    fn abort_operation(&mut self) {
        let command = match self.snap.state {
            State::Clean => return,
            State::Merging => "merge",
            State::Rebasing => "rebase",
            State::CherryPicking => "cherry-pick",
            State::Reverting => "revert",
        };
        let pending = self.pending(tr!("git.aborting"), After::Nothing, move |git| {
            git.run(&[command, "--abort"])
        });
        self.confirm(
            tr!("git.abort_operation"),
            tr!("git.changes_operation_progress_will_undone").into(),
            Vec::new(),
            pending,
        );
    }

    fn continue_operation(&mut self, host: &mut dyn Host) {
        let command = match self.snap.state {
            State::Merging => return self.commit(host, false),
            State::Rebasing => "rebase",
            State::CherryPicking => "cherry-pick",
            State::Reverting => "revert",
            State::Clean => return,
        };
        if !host.save_files() {
            return;
        }
        self.git(tr!("git.continuing"), &[command, "--continue"]);
    }

    // ---- Branches -----------------------------------------------------------------------

    fn checkout_args(target: &Target) -> Vec<String> {
        let args: &[&str] = match target {
            Target::Branch(name) => &["checkout", name],
            Target::Remote(name) => &["checkout", "--track", name],
            Target::Revision(rev) => &["checkout", "--detach", rev],
        };
        args.iter().map(|a| a.to_string()).collect()
    }

    fn checkout(&mut self, target: Target, host: &mut dyn Host) {
        if !host.save_files() {
            return;
        }
        if self.has_local_changes() {
            self.dialog = Some(Dialog::SmartCheckout { target });
        } else {
            self.run_checkout(target, false, false);
        }
    }

    fn run_checkout(&mut self, target: Target, smart: bool, force: bool) {
        let mut args = Self::checkout_args(&target);
        if force {
            args.insert(1, "--force".into());
        }
        self.spawn("Checkout", After::Nothing, move |git| {
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            if !smart {
                return git.run(&args);
            }
            git.run(&["stash", "push", "-m", SMART_STASH])?;
            let result = git.run(&args);
            // Restore the changes even when the checkout failed.
            let restored = git.run(&["stash", "pop"]);
            result?;
            restored
        });
    }

    fn create_branch(&mut self, name: String, from: Option<String>, checkout: bool) {
        if let Err(e) = repo::ensure_name(&name, tr!("git.branch_name")) {
            self.toast(e.to_string(), true);
            return;
        }
        self.spawn(tr!("git.new_branch_job"), After::Nothing, move |git| {
            let mut args = if checkout {
                vec!["checkout", "-b", &name]
            } else {
                vec!["branch", &name]
            };
            args.extend(from.as_deref());
            git.run(&args)
        });
    }

    fn delete_remote_branch(&mut self, name: String) {
        let Some((remote, branch)) = name.split_once('/') else {
            return;
        };
        let (remote, branch) = (remote.to_string(), branch.to_string());
        let pending = self.pending(
            tr!("git.deleting_remote_branch"),
            After::Nothing,
            move |git| git.run(&["push", &remote, "--delete", &branch]),
        );
        self.confirm(
            tr!("git.delete_remote_branch"),
            trf!("git.branch_will_deleted_from_server", name = name),
            Vec::new(),
            pending,
        );
    }

    fn merge(&mut self, name: String, host: &mut dyn Host) {
        if host.save_files() {
            self.git(
                &trf!("git.merging_name", name = name),
                &["merge", "--no-edit", &name],
            );
        }
    }

    fn rebase(&mut self, onto: String, host: &mut dyn Host) {
        if host.save_files() {
            self.git(&trf!("git.rebasing_onto", onto = onto), &["rebase", &onto]);
        }
    }

    // ---- Remote operations --------------------------------------------------------------

    fn fetch(&mut self) {
        self.git("Fetch", &["fetch", "--all", "--prune", "--progress"]);
    }

    fn open_update(&mut self) {
        if self.snap.head.upstream.is_none() {
            self.toast(tr!("git.current_branch_has_no_upstream"), true);
            return;
        }
        self.dialog = Some(Dialog::Update {
            rebase: false,
            autostash: true,
        });
    }

    fn update(&mut self, rebase: bool, autostash: bool, host: &mut dyn Host) {
        if !host.save_files() {
            return;
        }
        let mut args = vec![
            "pull",
            "--no-edit",
            "--progress",
            if rebase { "--rebase" } else { "--no-rebase" },
        ];
        if autostash {
            args.push("--autostash");
        }
        self.git(tr!("git.updating_project"), &args);
    }

    fn open_push(&mut self, host: &mut dyn Host) {
        let Some(branch) = self.current_branch().map(str::to_string) else {
            host.balloon(
                tr!("git.cannot_push"),
                tr!("git.detached_head_create_branch_before").into(),
                true,
            );
            return;
        };
        let upstream = self.snap.head.upstream.clone();
        let remote = upstream
            .as_deref()
            .and_then(|u| {
                self.snap
                    .remotes
                    .iter()
                    .find(|r| u.starts_with(&format!("{}/", r.name)))
            })
            .or_else(|| self.snap.remotes.iter().find(|r| r.name == "origin"))
            .or(self.snap.remotes.first())
            .map(|r| r.name.clone());
        let Some(remote) = remote else {
            self.toast(tr!("git.no_remotes_add_one_manage"), true);
            return;
        };
        let target = upstream
            .as_deref()
            .and_then(|u| u.strip_prefix(&format!("{remote}/")))
            .unwrap_or(&branch)
            .to_string();
        let commits = self
            .repo
            .as_ref()
            .and_then(|r| r.outgoing(upstream.as_deref()).ok())
            .unwrap_or_default();
        self.dialog = Some(Dialog::Push {
            remote,
            branch,
            target,
            force: false,
            tags: false,
            commits,
            upstream: upstream.is_some(),
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn push(
        &mut self,
        remote: String,
        branch: String,
        target: String,
        force: bool,
        tags: bool,
        upstream: bool,
    ) {
        self.spawn("Push", After::Nothing, move |git| {
            let refspec = format!("{branch}:{target}");
            let mut args = vec!["push", "--progress"];
            if force {
                args.push("--force-with-lease");
            }
            if tags {
                args.push("--tags");
            }
            if !upstream {
                args.push("--set-upstream");
            }
            args.extend([remote.as_str(), &refspec]);
            git.run(&args)
        });
    }

    // ---- Stash --------------------------------------------------------------------------

    fn stash(&mut self, message: String, untracked: bool, host: &mut dyn Host) {
        if !host.save_files() {
            return;
        }
        self.spawn(tr!("git.stashing"), After::Nothing, move |git| {
            let mut args = vec!["stash", "push"];
            if untracked {
                args.push("--include-untracked");
            }
            if !message.trim().is_empty() {
                args.extend(["-m", message.trim()]);
            }
            git.run(&args)
        });
    }

    fn select_stash(&mut self, name: String) {
        self.stash_files = self
            .repo
            .as_ref()
            .and_then(|r| r.changed_files(Some(&format!("{name}^1")), &name).ok())
            .unwrap_or_default();
        self.stash_selected = Some(name);
    }

    // ---- Log ----------------------------------------------------------------------------

    fn load_log(&mut self) {
        let Some(repo) = self.repo.clone() else {
            return;
        };
        if self.log.loading.is_some() || !self.loaded {
            return;
        }
        self.log.stale = false;
        if self.snap.head.oid.is_none() && self.snap.branches.is_empty() {
            self.log.rows.clear();
            return;
        }
        let query = self.log.query.clone();
        self.log.loading = Some(thread::spawn(move || repo.log(&query)));
    }

    fn show_history(&mut self, path: &Path, host: &mut dyn Host) {
        let Some(rel) = self.repo.as_ref().and_then(|r| r.rel(path)) else {
            return;
        };
        self.log.query.path = Some(rel);
        self.log.query.branch = None;
        self.log.stale = true;
        self.log_tab = LogTab::Log;
        host.show_tool_window(LOG_WINDOW);
    }

    fn select_commit(&mut self, hash: String) {
        let Some(repo) = &self.repo else { return };
        let parent = self
            .log
            .rows
            .iter()
            .find(|c| c.hash == hash)
            .and_then(|c| c.parents.first().cloned());
        let message = repo.message(&hash).unwrap_or_default();
        let files = repo
            .changed_files(parent.as_deref(), &hash)
            .unwrap_or_default();
        self.log.details = Some((hash.clone(), message, files));
        self.log.selected = Some(hash);
    }

    fn cherry_pick(&mut self, hash: String, merge: bool, host: &mut dyn Host) {
        if !host.save_files() {
            return;
        }
        let mut args = vec!["cherry-pick"];
        if merge {
            args.extend(["-m", "1"]);
        }
        args.push(&hash);
        self.git("Cherry-pick", &args);
    }

    fn revert(&mut self, hash: String, merge: bool, host: &mut dyn Host) {
        if !host.save_files() {
            return;
        }
        let mut args = vec!["revert", "--no-edit"];
        if merge {
            args.extend(["-m", "1"]);
        }
        args.push(&hash);
        self.git("Revert", &args);
    }

    fn reset(&mut self, rev: String, mode: ResetMode) {
        let flag = match mode {
            ResetMode::Soft => "--soft",
            ResetMode::Mixed => "--mixed",
            ResetMode::Hard => "--hard",
            ResetMode::Keep => "--keep",
        };
        self.git("Reset", &["reset", flag, &rev]);
    }

    fn undo_commit(&mut self) {
        let Some(repo) = &self.repo else { return };
        let message = repo.message("HEAD").unwrap_or_default();
        self.spawn(tr!("git.undoing_commit"), After::Message(message), |git| {
            git.run(&["reset", "--soft", "HEAD~1"])
        });
    }

    // ---- Diff, conflicts, blame ---------------------------------------------------------

    fn open_diff(&mut self, source: DiffSource) {
        let Some(repo) = &self.repo else { return };
        let (change, old, new, left_title, right_title) = match source {
            DiffSource::WorkingTree(change) => {
                let base = change.orig.as_deref().unwrap_or(&change.path);
                let old = repo.show("HEAD", base).unwrap_or_default();
                let new = std::fs::read(repo.abs(&change.path))
                    .map(|b| String::from_utf8_lossy(&b).into_owned())
                    .unwrap_or_default();
                (
                    change,
                    old,
                    new,
                    "HEAD".to_string(),
                    tr!("git.working_copy").to_string(),
                )
            }
            DiffSource::Revision { from, to, change } => {
                let base = change.orig.as_deref().unwrap_or(&change.path);
                let old = from
                    .as_deref()
                    .and_then(|f| repo.show(f, base))
                    .unwrap_or_default();
                let new = repo.show(&to, &change.path).unwrap_or_default();
                let short = |r: &str| r.get(..10).unwrap_or(r).to_string();
                let left = from.as_deref().map_or("(vide)".into(), short);
                (change, old, new, left, short(&to))
            }
        };
        let mut view = DiffView {
            path: change.path,
            left_title,
            right_title,
            binary: old.contains('\0') || new.contains('\0'),
            old,
            new,
            side_by_side: self.diff.as_ref().is_none_or(|d| d.side_by_side),
            collapse: self.diff.as_ref().is_some_and(|d| d.collapse),
            rows: Vec::new(),
            starts: Vec::new(),
            current: 0,
            scroll_to: None,
        };
        view.rebuild();
        self.diff = Some(view);
    }

    fn open_resolver(&mut self, path: String) {
        let Some(repo) = &self.repo else { return };
        match std::fs::read_to_string(repo.abs(&path)) {
            Ok(text) => {
                let segments = conflict::parse(&text);
                if conflict::conflicts(&segments) == 0 {
                    self.toast(tr!("git.no_conflict_markers_file_use"), false);
                }
                self.resolver = Some(Resolver { path, segments });
            }
            Err(e) => self.toast(trf!("git.cannot_read", e = e), true),
        }
    }

    fn apply_resolution(&mut self, host: &mut dyn Host) {
        let (Some(repo), Some(resolver)) = (&self.repo, &self.resolver) else {
            return;
        };
        let text = conflict::render(&resolver.segments);
        let done = conflict::resolved(&resolver.segments);
        let path = resolver.path.clone();
        if let Err(e) = std::fs::write(repo.abs(&path), text) {
            self.toast(trf!("git.cannot_write", e = e), true);
            return;
        }
        host.reload_files();
        self.resolver = None;
        if done {
            self.mark_resolved(path);
        } else {
            self.invalidate();
            self.toast(tr!("git.partial_resolution_saved"), false);
        }
    }

    fn toggle_annotations(&mut self, path: PathBuf) {
        if self.annotated.remove(&path) {
            return;
        }
        let Some(repo) = self.repo.clone() else {
            return;
        };
        let Some(rel) = repo.rel(&path) else { return };
        self.annotated.insert(path.clone());
        self.blame.remove(&path);
        if let std::collections::hash_map::Entry::Vacant(entry) = self.blame_jobs.entry(path) {
            entry.insert(thread::spawn(move || {
                let lines = repo.blame(&rel)?;
                Ok(lines
                    .iter()
                    .map(|l| {
                        if l.hash.bytes().all(|b| b == b'0') {
                            format!("{:<10}  {:<14}", "", tr!("git.not_committed"))
                        } else {
                            let author: String = l.author.chars().take(14).collect();
                            format!("{}  {author:<14}", repo::date(l.time, &l.tz))
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n"))
            }));
        }
    }

    fn change_at(&self, path: &Path) -> Option<Change> {
        let rel = self.repo.as_ref()?.rel(path)?;
        self.snap.changes.iter().find(|c| c.path == rel).cloned()
    }

    // ---- Repository creation ------------------------------------------------------------

    fn init(&mut self, host: &mut dyn Host) {
        let root = host.root().to_path_buf();
        let console = self.console.clone();
        self.spawn_task(
            tr!("git.creating_repository"),
            After::Reopen,
            Box::new(move || repo::Repo::init(&root, &console).map(|()| String::new())),
        );
    }

    fn clone_repo(&mut self, url: String, parent: String, name: String) {
        let url = url.trim().to_string();
        let name = if name.trim().is_empty() {
            url.trim_end_matches('/')
                .rsplit(['/', ':'])
                .next()
                .unwrap_or("")
                .trim_end_matches(".git")
                .to_string()
        } else {
            name.trim().to_string()
        };
        if url.is_empty() || name.is_empty() || parent.trim().is_empty() {
            self.toast(tr!("git.enter_url_destination_folder"), true);
            return;
        }
        let parent = PathBuf::from(parent.trim());
        let target = parent.join(&name);
        if target.exists() {
            self.toast(trf!("git.already_exists", target = target.display()), true);
            return;
        }
        let console = self.console.clone();
        self.spawn_task(
            tr!("git.cloning"),
            After::OpenProject(target),
            Box::new(move || {
                std::fs::create_dir_all(&parent)?;
                Repo::clone_into(&url, &parent, &name, &console)
            }),
        );
    }
}

impl BuiltinPlugin for GitPlugin {
    fn id(&self) -> &'static str {
        "git"
    }
    fn name(&self) -> &'static str {
        "Git"
    }
    fn description(&self) -> &'static str {
        tr!("git.commit_branches_history_diff_push")
    }
    fn tool_windows(&self) -> &'static [ToolWindow] {
        &TOOL_WINDOWS
    }
    fn icon(&self) -> &'static str {
        "git"
    }
    fn extensions_dir(&self) -> Option<&'static str> {
        Some("git")
    }

    fn update(&mut self, ctx: &egui::Context, host: &mut dyn Host) {
        self.poll(ctx, host);
    }

    fn shortcuts(&mut self, ctx: &egui::Context, host: &mut dyn Host) {
        use egui::{Key, Modifiers};
        let command = Modifiers::COMMAND;
        let pressed = |modifiers, key| ctx.input_mut(|i| i.consume_key(modifiers, key));
        if pressed(command | Modifiers::SHIFT, Key::K) {
            self.open_push(host);
        }
        if pressed(command, Key::K) {
            self.commit_tab = CommitTab::Changes;
            self.focus_message = true;
            host.show_tool_window(COMMIT_WINDOW);
        }
        if pressed(command, Key::T) {
            self.open_update();
        }
        if pressed(Modifiers::ALT, Key::Num0) {
            host.show_tool_window(COMMIT_WINDOW);
        }
        if pressed(Modifiers::ALT, Key::Num9) {
            host.show_tool_window(LOG_WINDOW);
        }
        if let Some(diff) = &mut self.diff {
            if pressed(Modifiers::SHIFT, Key::F7) {
                diff.step(false);
            }
            if pressed(Modifiers::NONE, Key::F7) {
                diff.step(true);
            }
        }
    }

    fn file_saved(&mut self, _path: &Path) {
        self.stale = true;
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, host: &mut dyn Host) {
        self.branch_widget(ui, host);
    }

    fn status_bar(&mut self, ui: &mut egui::Ui, host: &mut dyn Host) {
        self.status_widget(ui, host);
    }

    fn tool_window(&mut self, id: &str, ui: &mut egui::Ui, host: &mut dyn Host) {
        match id {
            "git.commit" => self.commit_window(ui, host),
            "git.log" => self.log_window(ui, host),
            _ => {}
        }
    }

    fn windows(&mut self, ctx: &egui::Context, host: &mut dyn Host) {
        self.diff_window(ctx);
        self.resolver_window(ctx, host);
        self.dialog_window(ctx, host);
        self.branch_popup_window(ctx, host);
    }

    fn commands(&self) -> Vec<Command> {
        let mut commands = vec![
            ("git.clone", tr!("git.git_clone_repository")),
            ("git.console", tr!("git.git_console")),
        ];
        if self.repo.is_none() {
            commands.push(("git.init", tr!("git.git_create_repository")));
        } else {
            commands.extend([
                ("git.commit", tr!("git.git_commit_ctrl_k")),
                ("git.push", tr!("git.git_push_ctrl_shift_k")),
                ("git.update", tr!("git.git_update_project_ctrl_t")),
                ("git.fetch", tr!("git.git_fetch")),
                ("git.branch", tr!("git.git_new_branch")),
                ("git.checkout", tr!("git.git_checkout_tag_revision")),
                ("git.stash", tr!("git.git_stash_changes")),
                ("git.log", tr!("git.git_log_alt_9")),
                ("git.history", tr!("git.git_history_current_file")),
                ("git.annotate", tr!("git.git_annotate_current_file_blame")),
                ("git.remotes", tr!("git.git_manage_remotes")),
            ]);
        }
        commands
            .into_iter()
            .map(|(id, label)| Command {
                id,
                label: label.into(),
            })
            .collect()
    }

    fn run_command(&mut self, id: &str, host: &mut dyn Host) {
        let active = host.active_file();
        match id {
            "git.clone" => {
                let parent = host
                    .root()
                    .parent()
                    .map(|p| repo::simplify(p).display().to_string())
                    .unwrap_or_default();
                self.dialog = Some(Dialog::Clone {
                    url: String::new(),
                    parent,
                    name: String::new(),
                });
            }
            "git.init" => self.init(host),
            "git.console" => {
                self.log_tab = LogTab::Console;
                host.show_tool_window(LOG_WINDOW);
            }
            "git.commit" => {
                self.commit_tab = CommitTab::Changes;
                self.focus_message = true;
                host.show_tool_window(COMMIT_WINDOW);
            }
            "git.push" => self.open_push(host),
            "git.update" => self.open_update(),
            "git.fetch" => self.fetch(),
            "git.branch" => {
                self.dialog = Some(Dialog::NewBranch {
                    from: None,
                    name: String::new(),
                    checkout: true,
                })
            }
            "git.checkout" => self.dialog = Some(Dialog::Revision { rev: String::new() }),
            "git.stash" => {
                self.dialog = Some(Dialog::Stash {
                    message: String::new(),
                    untracked: false,
                })
            }
            "git.log" => {
                self.log_tab = LogTab::Log;
                host.show_tool_window(LOG_WINDOW);
            }
            "git.remotes" => {
                self.dialog = Some(Dialog::Remotes {
                    name: String::new(),
                    url: String::new(),
                })
            }
            "git.history" | "git.annotate" | "git.file_diff" | "git.file_rollback" => {
                let Some(path) = active else {
                    host.notify(tr!("git.open_file_repository").into());
                    return;
                };
                match id {
                    "git.history" => self.show_history(&path, host),
                    "git.annotate" => self.toggle_annotations(path),
                    "git.file_diff" => match self.change_at(&path) {
                        Some(change) => self.open_diff(DiffSource::WorkingTree(change)),
                        None => host.notify(tr!("git.no_changes_compared_head").into()),
                    },
                    _ => {
                        if host.save_files() {
                            let change = self.change_at(&path);
                            self.rollback(change.into_iter().collect());
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn ignored_by(&self, path: &Path) -> Option<String> {
        self.is_ignored(path).then(|| ".gitignore".to_string())
    }

    fn file_color(&self, path: &Path) -> Option<Color32> {
        match self.colors.get(path) {
            Some(kind) => Some(status_color(&self.theme, *kind)),
            None if self.changed_dirs.contains(path) => {
                Some(status_color(&self.theme, Kind::Modified))
            }
            None => self
                .is_ignored(path)
                .then_some(Color32::from_rgb(222, 140, 52)),
        }
    }

    fn line_marks(&mut self, path: &Path, text: &str) -> Vec<LineMark> {
        let Some(repo) = &self.repo else {
            return Vec::new();
        };
        if !self.bases.contains_key(path) {
            let Some(rel) = repo.rel(path) else {
                return Vec::new();
            };
            let base = repo.show("HEAD", &rel).or_else(|| {
                // A new file added to Git: every line is an addition.
                self.snap
                    .changes
                    .iter()
                    .any(|c| c.path == rel && c.kind == Kind::Added)
                    .then(String::new)
            });
            self.bases.insert(path.to_path_buf(), base);
        }
        let Some(Some(base)) = self.bases.get(path) else {
            return Vec::new();
        };
        if let Some((cached, marks)) = self.marks.get(path)
            && cached == text
        {
            return marks.clone();
        }
        let marks = diff::line_marks(base, text);
        self.marks
            .insert(path.to_path_buf(), (text.to_string(), marks.clone()));
        marks
    }

    fn annotations(&mut self, path: &Path) -> Option<String> {
        if !self.annotated.contains(path) {
            return None;
        }
        if let Some(text) = self.blame.get(path) {
            return Some(text.clone());
        }
        if !self.blame_jobs.contains_key(path) {
            // Invalidated by an operation: compute again.
            self.annotated.remove(path);
            self.toggle_annotations(path.to_path_buf());
        }
        Some("Chargement…".into())
    }

    fn gutter_commands(&self, path: &Path) -> Vec<Command> {
        let Some(repo) = &self.repo else {
            return Vec::new();
        };
        if repo.rel(path).is_none() {
            return Vec::new();
        }
        let annotate = if self.annotated.contains(path) {
            tr!("git.hide_git_annotations")
        } else {
            tr!("git.annotate_git_blame")
        };
        let mut commands = vec![
            Command {
                id: "git.annotate",
                label: annotate.into(),
            },
            Command {
                id: "git.history",
                label: tr!("git.file_history").into(),
            },
        ];
        if self.colors.contains_key(path) {
            commands.extend([
                Command {
                    id: "git.file_diff",
                    label: tr!("git.show_diff_head").into(),
                },
                Command {
                    id: "git.file_rollback",
                    label: tr!("git.rollback_file").into(),
                },
            ]);
        }
        commands
    }
}

#[cfg(test)]
#[path = "../../../../test/plugins/builtins/git/plugin/tests.rs"]
mod tests;
