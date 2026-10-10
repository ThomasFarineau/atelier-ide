use super::*;
use std::{path::PathBuf, process::Command};

fn git(dir: &Path, args: &[&str]) {
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
}

fn repo_with(name: &str, history: &[&str]) -> PathBuf {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir =
        std::env::temp_dir().join(format!("atelier-commits-{name}-{}-{n}", std::process::id()));
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
    let out = Command::new("git")
        .current_dir(dir)
        .args(["log", "--format=%B%x1e"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout)
        .split('\u{1e}')
        .map(|m| m.trim().to_string())
        .filter(|m| !m.is_empty())
        .collect()
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
    assert_eq!(
        (c.style, c.french, c.capitalized, c.period),
        (Style::Conventional, true, true, false)
    );
    assert_eq!(c.types[0], ("feat".to_string(), 2));
    assert!(c.scope_rate > 0.5 && c.scopes.iter().any(|(s, _)| s == "editor"));
    let english: Vec<String> = [
        "Add folding",
        "Fix the terminal split",
        "Update the readme",
        "Remove dead code",
    ]
    .map(String::from)
    .to_vec();
    let c = detect_from(&english);
    assert_eq!((c.style, c.french), (Style::Free, false));
    let moji: Vec<String> = ["✨ add folding", "🐛 fix split", "📝 docs", "🎨 style"]
        .map(String::from)
        .to_vec();
    assert_eq!(detect_from(&moji).style, Style::Gitmoji);
    assert_eq!(detect_from(&[]).samples, 0);
    assert!(
        guidance(&detect_from(&french)).contains("Conventional Commits")
            && guidance(&detect_from(&french)).contains("French")
    );
}

#[test]
fn messages_are_checked_against_the_conventions_of_the_repository() {
    let conventional = detect_from(&[
        "feat(a): x".to_string(),
        "fix: y".to_string(),
        "docs: z".to_string(),
    ]);
    assert!(check(&conventional, "feat(ui): add a button").is_ok());
    assert!(
        check(&conventional, "Add a button")
            .unwrap_err()
            .contains("not a Conventional Commit")
    );
    assert!(
        check(&conventional, "wibble: add a button")
            .unwrap_err()
            .contains("not used")
    );
    assert!(
        check(&conventional, &format!("feat: {}", "x".repeat(120)))
            .unwrap_err()
            .contains("characters")
    );
    assert!(check(&conventional, "  ").is_err());
    let free = detect_from(
        &(0..12)
            .map(|i| format!("Add thing {i}"))
            .collect::<Vec<_>>(),
    );
    assert!(check(&free, "Add another thing").is_ok());
    assert!(
        check(&free, "feat: add another thing")
            .unwrap_err()
            .contains("plain sentence")
    );
}

#[test]
fn co_authors_and_ai_attributions_never_stay_in_a_message() {
    let message = "feat: ajoute x\n\nLe détail.\n\nCo-Authored-By: Claude Sonnet <noreply@anthropic.com>\n🤖 Generated with [Claude Code](https://claude.com/claude-code)\nco-authored-by: Autre <a@b.c>\n";
    assert_eq!(clean_message(message), "feat: ajoute x\n\nLe détail.");
    assert_eq!(
        clean_message("fix: y\r\n\r\nCo-authored-by: x <y@z>"),
        "fix: y"
    );
    assert_eq!(
        clean_message("docs: explain how a Generated with caching works"),
        "docs: explain how a Generated with caching works",
        "ordinary text is left alone"
    );
}

fn change(path: &str, kind: Kind) -> Change {
    Change {
        path: path.into(),
        orig: None,
        kind,
    }
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
    let code: Vec<Vec<&str>> = groups
        .iter()
        .filter(|g| g.kind == "code")
        .map(|g| g.files.iter().map(|c| c.path.as_str()).collect())
        .collect();
    assert_eq!(code.len(), 2, "{code:?}");
    assert!(
        code.iter()
            .any(|g| g == &["src/graph/mod.rs", "src/graph/build.rs"])
    );
    assert!(
        code.iter()
            .any(|g| g.contains(&"src/editor.rs") && g.contains(&"src/ui.rs")),
        "linked files stay together: {code:?}"
    );
    // Every file is in exactly one group.
    assert_eq!(
        groups.iter().map(|g| g.files.len()).sum::<usize>(),
        changes.len()
    );
    // Many folders do not make many commits.
    let many: Vec<Change> = (0..20)
        .map(|i| change(&format!("dir{i}/f.rs"), Kind::Modified))
        .collect();
    assert!(group(&many, &[]).len() <= MAX_GROUPS);
    assert_eq!(
        suggested_type(&Group {
            title: String::new(),
            kind: "code",
            files: vec![change("a", Kind::Untracked)]
        }),
        Some("feat")
    );
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
    let refused = commit(
        &dir,
        &json!({"mode": "all", "commits": [{"message": "Add stuff"}]}),
    )
    .unwrap_err();
    assert!(refused.contains("Conventional Commit"), "{refused}");
    assert_eq!(subjects(&dir).len(), 3);
    let done = commit(&dir, &json!({"mode": "all", "commits": [{"message": "feat: add new.rs\n\nCo-Authored-By: Claude <noreply@anthropic.com>"}]})).unwrap();
    assert!(
        done.contains("1 commit(s) made") && done.contains("clean"),
        "{done}"
    );
    let log = subjects(&dir);
    assert_eq!(log[0], "feat: add new.rs", "{log:?}");
    assert!(!log[0].to_lowercase().contains("co-authored"));
    assert!(plan_nothing(&dir));
    let _ = std::fs::remove_dir_all(&dir);
}

fn plan_nothing(dir: &Path) -> bool {
    plan(dir, &json!({"mode": "all"}))
        .unwrap_err()
        .contains("Nothing to commit")
}

#[test]
fn by_context_makes_one_commit_per_group_and_leaves_the_rest() {
    let dir = repo_with("context", &["feat: un", "fix: deux", "docs: trois"]);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("src/a.rs"), "a").unwrap();
    std::fs::write(dir.join("src/b.rs"), "b").unwrap();
    std::fs::write(dir.join("README.md"), "docs").unwrap();
    std::fs::write(dir.join("h1.txt"), "left alone").unwrap();
    let groups: Value =
        serde_json::from_str(&plan(&dir, &json!({"mode": "context"})).unwrap()).unwrap();
    let kinds: Vec<&str> = groups["groups"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|g| g["kind"].as_str())
        .collect();
    assert!(
        kinds.contains(&"docs") && kinds.contains(&"code"),
        "{kinds:?}"
    );
    // Missing files, unknown files and a file in two commits are all refused up front.
    for bad in [
        json!([{"message": "feat: x"}]),
        json!([{"message": "feat: x", "files": ["nope.rs"]}]),
        json!([{"message": "feat: x", "files": ["src/a.rs"]}, {"message": "fix: y", "files": ["src/a.rs"]}]),
    ] {
        assert!(commit(&dir, &json!({"mode": "context", "commits": bad})).is_err());
    }
    assert_eq!(
        subjects(&dir).len(),
        3,
        "nothing was committed by the refused calls"
    );
    let done = commit(
        &dir,
        &json!({"mode": "context", "commits": [
            {"message": "feat(src): add a and b", "files": ["src/a.rs", "src/b.rs"]},
            {"message": "docs: describe the change", "files": ["README.md"]},
        ]}),
    )
    .unwrap();
    assert!(
        done.contains("2 commit(s) made") && done.contains("1 changed file(s) remain"),
        "{done}"
    );
    let log = subjects(&dir);
    assert_eq!(
        &log[..2],
        ["docs: describe the change", "feat(src): add a and b"]
    );
    let status = Command::new("git")
        .current_dir(&dir)
        .args(["status", "--porcelain"])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&status.stdout).trim(),
        "M h1.txt",
        "only the file nobody listed is left"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_commit_command_is_offered_with_its_two_modes() {
    let names: Vec<String> = prompts()
        .iter()
        .filter_map(|p| p["name"].as_str().map(str::to_string))
        .collect();
    assert_eq!(names, ["commit", "commit-all", "commit-by-context"]);
    assert!(
        prompt("commit-all", &json!({}))
            .unwrap()
            .contains("“all” mode")
    );
    assert!(
        prompt("commit", &json!({"mode": "context"}))
            .unwrap()
            .contains("“context” mode")
    );
    assert!(
        prompt("commit", &json!({}))
            .unwrap()
            .contains("ask the user which mode")
    );
    let text = prompt("commit-by-context", &json!({})).unwrap();
    assert!(
        text.contains("never add a `Co-authored-by`")
            && text.contains("git_commit_plan")
            && text.contains("git_commit")
    );
    assert_eq!(prompt("autre", &json!({})), None);
}
