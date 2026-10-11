use super::*;

/// A WSL distribution with git, if this machine has one.
fn wsl_distro_with_git() -> Option<String> {
    let shell = crate::terminal::Shell::available()
        .into_iter()
        .find(crate::terminal::is_wsl)?;
    let distro = shell.args.get(1)?.clone();
    let ok = Command::new("wsl.exe")
        .args(["-d", &distro, "--exec", "git", "--version"])
        .output()
        .is_ok_and(|o| o.status.success());
    ok.then_some(distro)
}

/// A folder of a WSL distribution is handled by the git inside it, and its Linux paths come
/// back as Windows paths of the share. Skipped on machines without WSL.
#[test]
fn a_repository_in_wsl_is_read_by_the_git_of_the_distribution() {
    let Some(distro) = wsl_distro_with_git() else {
        return;
    };
    let path = crate::wsl::to_windows(
        &distro,
        &format!("/tmp/atelier-git-wsl-{}", std::process::id()),
    );
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    let console = Console::default();
    Repo::init(&path, &console).unwrap();
    let repo = Repo::open(&path, console).unwrap().unwrap();
    for (key, value) in [
        ("user.name", "Atelier Test"),
        ("user.email", "atelier@example.com"),
    ] {
        repo.run(&["config", key, value]).unwrap();
    }
    std::fs::write(
        repo.abs("a.txt"),
        "one
",
    )
    .unwrap();
    let (_, changes) = repo.status().unwrap();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].path, "a.txt");
    repo.run(&["add", "-A"]).unwrap();
    repo.run(&["commit", "-m", "init"]).unwrap();
    assert_eq!(
        repo.log(&LogQuery {
            limit: 10,
            ..Default::default()
        })
        .unwrap()
        .len(),
        1
    );
    assert_eq!(repo.rel(&path.join("a.txt")).as_deref(), Some("a.txt"));
    assert!(
        crate::wsl::parse(&repo.root).is_some_and(|w| w.distro == distro),
        "{:?}",
        repo.root
    );
    let _ = std::fs::remove_dir_all(&path);
}

/// Temporary repository with a local identity, deleted on drop.
pub struct TempRepo {
    pub repo: Repo,
    pub path: PathBuf,
}

impl TempRepo {
    pub fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("atelier-git-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        let console = Console::default();
        Repo::init(&path, &console).unwrap();
        let repo = Repo::open(&path, console).unwrap().unwrap();
        for (key, value) in [
            ("user.name", "Atelier Test"),
            ("user.email", "atelier@example.com"),
            ("commit.gpgsign", "false"),
            ("core.autocrlf", "false"),
        ] {
            repo.run(&["config", key, value]).unwrap();
        }
        Self { repo, path }
    }
    pub fn write(&self, name: &str, text: &str) {
        let path = self.repo.abs(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    pub fn commit_all(&self, message: &str) {
        self.repo.run(&["add", "-A"]).unwrap();
        self.repo.run(&["commit", "-q", "-m", message]).unwrap();
    }
}

impl Drop for TempRepo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

#[test]
fn parses_status_entries() {
    let out = "# branch.oid abc\0# branch.head main\0# branch.upstream origin/main\0# branch.ab +2 -1\0\
            1 .M N... 100644 100644 100644 a b src/a b.rs\0\
            2 R. N... 100644 100644 100644 a b R100 new.rs\0old.rs\0\
            u UU N... 100644 100644 100644 100644 a b c conflict.rs\0\
            1 D. N... 100644 000000 000000 a b gone.rs\0\
            ? notes.txt\0";
    let (head, changes) = parse_status(out);
    assert_eq!(head.branch.as_deref(), Some("main"));
    assert_eq!((head.ahead, head.behind), (2, 1));
    let summary: Vec<_> = changes
        .iter()
        .map(|c| (c.path.as_str(), c.orig.as_deref(), c.kind))
        .collect();
    assert_eq!(
        summary,
        [
            ("conflict.rs", None, Kind::Conflicted),
            ("gone.rs", None, Kind::Deleted),
            ("new.rs", Some("old.rs"), Kind::Renamed),
            ("notes.txt", None, Kind::Untracked),
            ("src/a b.rs", None, Kind::Modified),
        ]
    );
}

#[test]
fn parses_log_and_refs() {
    let out = "h2\x1fh1 h0\x1fAda\x1fa@x\x1f01/01/2026 10:00\x1fHEAD -> refs/heads/main, refs/remotes/origin/main, refs/remotes/origin/HEAD, tag: refs/tags/v1\x1fMerge: a\x1fb\nh1\x1f\x1fAda\x1fa@x\x1f01/01/2026 09:00\x1f\x1fInit\n";
    let rows = parse_log(out);
    assert_eq!(rows.len(), 2);
    let commit = &rows[0];
    assert_eq!(commit.parents, ["h1", "h0"]);
    let refs: Vec<_> = commit
        .refs
        .iter()
        .map(|r| (r.name.as_str(), r.kind, r.current))
        .collect();
    assert_eq!(
        refs,
        [
            ("main", RefKind::Local, true),
            ("origin/main", RefKind::Remote, false),
            ("v1", RefKind::Tag, false),
        ]
    );
    // The subject keeps the separator it may contain.
    assert_eq!(commit.subject, "Merge: a\x1fb");
    assert!(rows[1].parents.is_empty() && rows[1].refs.is_empty());
    assert_eq!(rows[1].subject, "Init");
}

#[test]
fn formats_dates_in_author_time_zone() {
    assert_eq!(date(0, "+0000"), "1970-01-01");
    // 2024-02-29 23:30 UTC is already March 1st in Paris.
    assert_eq!(date(1_709_249_400, "+0100"), "2024-03-01");
    assert_eq!(date(1_709_249_400, "-0500"), "2024-02-29");
}

#[test]
fn status_branches_log_and_blame_on_a_real_repository() {
    let t = TempRepo::new("backend");
    t.write("a.txt", "one\ntwo\n");
    t.write("dir/b.txt", "b\n");
    t.commit_all("first");
    t.write("a.txt", "one\n2\n");
    t.write("new file.txt", "n\n");
    let (head, changes) = t.repo.status().unwrap();
    assert!(head.oid.is_some());
    let kinds: Vec<_> = changes.iter().map(|c| (c.path.as_str(), c.kind)).collect();
    assert_eq!(
        kinds,
        [("a.txt", Kind::Modified), ("new file.txt", Kind::Untracked)]
    );
    assert_eq!(t.repo.show("HEAD", "a.txt").unwrap(), "one\ntwo\n");
    assert!(t.repo.show("HEAD", "missing").is_none());
    let blame = t.repo.blame("dir/b.txt").unwrap();
    assert_eq!(blame.len(), 1);
    assert_eq!(blame[0].author, "Atelier Test");
    assert_eq!(blame[0].summary, "first");
    t.repo.run(&["branch", "feature"]).unwrap();
    let branches = t.repo.branches().unwrap();
    assert_eq!(branches.len(), 2);
    assert!(branches.iter().any(|b| b.current && !b.remote));
    let log = t
        .repo
        .log(&LogQuery {
            limit: 10,
            ..Default::default()
        })
        .unwrap();
    let commit = &log[0];
    assert_eq!(commit.subject, "first");
    assert!(commit.refs.iter().any(|r| r.name == "feature"));
    let files = t.repo.changed_files(None, &commit.hash).unwrap();
    assert_eq!(files.len(), 2);
    assert_eq!(t.repo.rel(&t.repo.abs("dir/b.txt")).unwrap(), "dir/b.txt");
    assert!(!t.repo.console.lock().unwrap().is_empty());
}
