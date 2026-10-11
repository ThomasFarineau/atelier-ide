use super::*;
use crate::plugins::builtins::git::repo::tests::TempRepo;

fn wait(plugin: &mut GitPlugin) -> Result<String> {
    let job = plugin.job.take().expect("a job was started");
    job.handle.join().unwrap()
}

fn plugin(t: &TempRepo) -> GitPlugin {
    let mut plugin = GitPlugin {
        repo: Some(t.repo.clone()),
        ..Default::default()
    };
    plugin.apply(load(&t.repo).unwrap());
    plugin
}

struct NoHost(PathBuf);
impl Host for NoHost {
    fn root(&self) -> &Path {
        &self.0
    }
    fn active_file(&self) -> Option<PathBuf> {
        None
    }
    fn open_file(&mut self, _: PathBuf) {}
    fn open_project(&mut self, _: PathBuf) {}
    fn save_files(&mut self) -> bool {
        true
    }
    fn reload_files(&mut self) {}
    fn show_tool_window(&mut self, _: &'static ToolWindow) {}
    fn notify(&mut self, _: String) {}
}

#[test]
fn commits_only_checked_files_including_renames() {
    let t = TempRepo::new("commit");
    t.write("keep.txt", "k\n");
    t.write("old.txt", "o\n");
    t.commit_all("init");
    t.write("keep.txt", "changed\n");
    t.repo.run(&["mv", "old.txt", "renamed.txt"]).unwrap();
    t.write("new.txt", "n\n");
    let mut host = NoHost(t.repo.root.clone());
    let mut plugin = plugin(&t);
    // Like IntelliJ: tracked changes are checked, new files are not.
    let keep = plugin
        .snap
        .changes
        .iter()
        .find(|c| c.path == "keep.txt")
        .cloned()
        .unwrap();
    plugin.set_included(&keep, false);
    plugin.message = "rename".into();
    plugin.commit(&mut host, false);
    wait(&mut plugin).unwrap();
    let (_, changes) = t.repo.status().unwrap();
    let left: Vec<_> = changes.iter().map(|c| (c.path.as_str(), c.kind)).collect();
    assert_eq!(
        left,
        [("keep.txt", Kind::Modified), ("new.txt", Kind::Untracked)]
    );
    let files = t.repo.changed_files(Some("HEAD~1"), "HEAD").unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].kind, Kind::Renamed);
}

#[test]
fn rollback_restores_tracked_files_and_unstages_added_ones() {
    let t = TempRepo::new("rollback");
    t.write("a.txt", "a\n");
    t.commit_all("init");
    t.write("a.txt", "edited\n");
    t.write("b.txt", "b\n");
    t.repo.run(&["add", "b.txt"]).unwrap();
    let mut plugin = plugin(&t);
    plugin.rollback(plugin.snap.changes.clone());
    let Some(Dialog::Confirm { pending, .. }) = plugin.dialog.take() else {
        panic!("rollback asks for confirmation");
    };
    (pending.task)().unwrap();
    assert_eq!(std::fs::read_to_string(t.repo.abs("a.txt")).unwrap(), "a\n");
    let (_, changes) = t.repo.status().unwrap();
    assert_eq!(changes.len(), 1);
    assert_eq!(
        changes[0].kind,
        Kind::Untracked,
        "the added file stays on disk"
    );
}

#[test]
fn smart_checkout_keeps_local_changes() {
    let t = TempRepo::new("smart");
    t.write("a.txt", "a\n");
    t.commit_all("init");
    t.repo.run(&["branch", "other"]).unwrap();
    t.write("a.txt", "local\n");
    let mut plugin = plugin(&t);
    let mut host = NoHost(t.repo.root.clone());
    plugin.checkout(Target::Branch("other".into()), &mut host);
    let Some(Dialog::SmartCheckout { target }) = plugin.dialog.take() else {
        panic!("local changes trigger the smart checkout dialog");
    };
    plugin.run_checkout(target, true, false);
    wait(&mut plugin).unwrap();
    let (head, changes) = t.repo.status().unwrap();
    assert_eq!(head.branch.as_deref(), Some("other"));
    assert_eq!(changes.len(), 1);
    assert_eq!(
        std::fs::read_to_string(t.repo.abs("a.txt")).unwrap(),
        "local\n"
    );
}

/// Draws every view, popup and dialog for a few frames: nothing may panic.
#[test]
fn every_view_and_dialog_renders() {
    let t = TempRepo::new("render");
    t.write("src/a.rs", "fn a() {}\n");
    t.commit_all("init");
    t.repo.run(&["branch", "feature"]).unwrap();
    t.write("src/a.rs", "fn a() { b(); }\n");
    t.write("new.txt", "n\n");
    let mut host = NoHost(t.repo.root.clone());
    let mut plugin = plugin(&t);
    plugin.project = Some(t.repo.root.clone());
    plugin.log.rows = t.repo.log(&plugin.log.query).unwrap();
    plugin.log.graph = graph::layout(
        plugin
            .log
            .rows
            .iter()
            .map(|c| (c.hash.as_str(), c.parents.as_slice())),
    );
    plugin.log.stale = false;
    let hash = plugin.log.rows[0].hash.clone();
    plugin.select_commit(hash.clone());
    let change = plugin.snap.changes[0].clone();
    plugin.open_diff(DiffSource::WorkingTree(change));
    plugin.resolver = Some(Resolver {
        path: "src/a.rs".into(),
        segments: conflict::parse("a\n<<<<<<< HEAD\nb\n=======\nc\n>>>>>>> feature\n"),
    });
    let current = plugin
        .snap
        .branches
        .iter()
        .find(|b| b.current)
        .cloned()
        .unwrap();
    plugin.branch_popup = Some((egui::pos2(10.0, 10.0), egui::Align2::LEFT_TOP, 0));
    plugin.branch_sub = Some((current, egui::pos2(380.0, 40.0)));
    let ctx = egui::Context::default();
    ui::install(&ctx, ui::theme_by_index(0), 14.0);
    let frame = |plugin: &mut GitPlugin, host: &mut NoHost| {
        for _ in 0..2 {
            let _ = ctx.run(Default::default(), |ctx| {
                egui::TopBottomPanel::top("bar").show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        plugin.toolbar(ui, host);
                        plugin.status_bar(ui, host);
                    });
                });
                egui::SidePanel::left("commit")
                    .show(ctx, |ui| plugin.tool_window("git.commit", ui, host));
                egui::CentralPanel::default()
                    .show(ctx, |ui| plugin.tool_window("git.log", ui, host));
                plugin.windows(ctx, host);
            });
        }
    };
    frame(&mut plugin, &mut host);
    plugin.commit_tab = CommitTab::Stash;
    plugin.log_tab = LogTab::Console;
    frame(&mut plugin, &mut host);
    plugin.rollback(plugin.snap.changes.clone());
    assert!(matches!(plugin.dialog, Some(Dialog::Confirm { .. })));
    frame(&mut plugin, &mut host);
    let dialogs = [
        Dialog::NewBranch {
            from: Some(hash.clone()),
            name: String::new(),
            checkout: true,
        },
        Dialog::RenameBranch {
            old: "feature".into(),
            name: "feature".into(),
        },
        Dialog::DeleteBranch {
            name: "feature".into(),
            force: false,
        },
        Dialog::NewTag {
            rev: hash.clone(),
            name: String::new(),
            message: String::new(),
        },
        Dialog::Revision { rev: String::new() },
        Dialog::SmartCheckout {
            target: Target::Branch("feature".into()),
        },
        Dialog::Push {
            remote: "origin".into(),
            branch: "main".into(),
            target: "main".into(),
            force: false,
            tags: false,
            commits: vec!["abc1234  init".into()],
            upstream: false,
        },
        Dialog::Update {
            rebase: false,
            autostash: true,
        },
        Dialog::Stash {
            message: String::new(),
            untracked: false,
        },
        Dialog::Reset {
            rev: hash.clone(),
            subject: "init".into(),
            mode: ResetMode::Mixed,
        },
        Dialog::Clone {
            url: String::new(),
            parent: String::new(),
            name: String::new(),
        },
        Dialog::Remotes {
            name: String::new(),
            url: String::new(),
        },
    ];
    for dialog in dialogs {
        plugin.dialog = Some(dialog);
        frame(&mut plugin, &mut host);
        assert!(plugin.dialog.is_some(), "a dialog stays open without input");
    }
    assert!(plugin.job.is_none());
}

#[test]
fn gutter_marks_and_tree_colors() {
    let t = TempRepo::new("marks");
    t.write("src/a.txt", "1\n2\n3\n");
    t.commit_all("init");
    t.write("src/a.txt", "1\ntwo\n3\n");
    let mut plugin = plugin(&t);
    let path = t.repo.abs("src/a.txt");
    let marks = plugin.line_marks(&path, "1\ntwo\n3\nfour\n");
    assert_eq!(marks.len(), 2);
    assert_eq!(plugin.file_color(&path), Some(plugin.theme.modified));
    assert!(plugin.file_color(&t.repo.abs("src")).is_some());
    assert!(plugin.file_color(&t.repo.abs("other")).is_none());
}

#[test]
fn ignored_files_and_folders_are_orange() {
    let t = TempRepo::new("ignored");
    t.write(
        ".gitignore",
        "target/
*.log
",
    );
    t.write(
        "a.txt", "1
",
    );
    t.commit_all("init");
    t.write("target/debug/x.bin", "x");
    t.write("run.log", "x");
    let plugin = plugin(&t);
    let orange = plugin.file_color(&t.repo.abs("run.log"));
    assert!(orange.is_some());
    assert_eq!(plugin.file_color(&t.repo.abs("target")), orange);
    assert_eq!(plugin.file_color(&t.repo.abs("target/debug/x.bin")), orange);
    assert!(plugin.file_color(&t.repo.abs("a.txt")).is_none());
}
