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
        serde_json::from_str(&plan(&dir, &json!({"range": "last 5 commits"})).unwrap()).unwrap();
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
        .map(|l| l.split_once('|').unwrap().1.to_string())
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
    let remote = std::env::temp_dir().join(format!("atelier-rework-remote-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&remote);
    git(
        &std::env::temp_dir(),
        &["init", "-q", "--bare", remote.to_str().unwrap()],
    );
    git(&dir, &["remote", "add", "origin", remote.to_str().unwrap()]);
    git(&dir, &["push", "-q", "origin", "HEAD:refs/heads/main"]);
    git(&dir, &["fetch", "-q", "origin"]);
    let plan: Value = serde_json::from_str(&plan(&dir, &json!({"range": "2"})).unwrap()).unwrap();
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
