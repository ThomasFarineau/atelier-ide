use super::*;
use crate::languages::{DocRef, Provider, json};

#[test]
fn reads_the_pull_request_and_its_reviews() {
    let view = serde_json::json!({
        "number": 21, "title": "Cargo plugin", "url": "https://github.com/o/r/pull/21",
        "state": "OPEN", "isDraft": false, "reviewDecision": "CHANGES_REQUESTED",
        "author": { "login": "thomas" }, "headRefName": "feat/x", "baseRefName": "main",
        "reviews": [{ "author": { "login": "ana" }, "state": "CHANGES_REQUESTED",
                      "body": "A few things", "submittedAt": "2026-10-16T09:12:00Z" }]
    });
    let pull = pull::read_pull(&view);
    assert_eq!(pull.number, 21);
    assert_eq!(pull.decision, "CHANGES_REQUESTED");
    assert_eq!(pull.reviews[0].author, "ana");
    assert_eq!(day(&pull.reviews[0].date), "2026-10-16");
}

#[test]
fn review_comments_become_conversations_across_pages() {
    let pages = r#"[{"id": 1, "path": "src/b.rs", "line": 4, "body": "Why?", "user": {"login": "ana"},
                    "created_at": "2026-10-16T09:00:00Z", "html_url": "u1"},
                   {"id": 2, "path": "src/a.rs", "line": null, "body": "Old", "user": {"login": "ana"},
                    "created_at": "2026-10-16T09:00:00Z", "html_url": "u2"}]
                  [{"id": 3, "in_reply_to_id": 1, "path": "src/b.rs", "line": 4, "body": "Because",
                    "user": {"login": "thomas"}, "created_at": "2026-10-16T10:00:00Z"},
                   {"id": 4, "in_reply_to_id": 3, "path": "src/b.rs", "line": 4, "body": "Ok",
                    "user": {"login": "ana"}, "created_at": "2026-10-16T11:00:00Z"}]"#;
    let threads = pull::threads(pages);
    assert_eq!(threads.len(), 2);
    assert_eq!(threads[0].path, "src/a.rs");
    assert_eq!(threads[0].line, None);
    let bodies: Vec<&str> = threads[1]
        .comments
        .iter()
        .map(|c| c.body.as_str())
        .collect();
    assert_eq!(bodies, ["Why?", "Because", "Ok"]);
}

#[test]
fn is_a_sub_plugin_of_git_on_github_projects() {
    let plugin = GitHubPlugin::default();
    assert_eq!(plugin.parent(), Some("git"));
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    assert!(on_github(root));
    assert!(!on_github(
        &std::env::temp_dir().join("atelier-no-such-project")
    ));
}

fn diagnostics(path: &Path, text: &str) -> Vec<(String, String)> {
    let registry = json::Registry::default();
    registry.set(SCHEMAS.iter().collect());
    let mut provider = json::Json::new(registry);
    let doc = DocRef {
        key: 1,
        path: Some(path.to_path_buf()),
        language: "yaml".into(),
    };
    provider.update(&doc, text, 1);
    provider
        .diagnostics(1)
        .iter()
        .map(|d| (text[d.range.clone()].to_string(), d.message.clone()))
        .collect()
}

/// The workflows and the Dependabot file of this repository follow their schemas.
#[test]
fn the_files_of_this_repository_follow_the_schemas() {
    let github = Path::new(env!("CARGO_MANIFEST_DIR")).join(".github");
    let mut files = vec![github.join("dependabot.yml")];
    for entry in std::fs::read_dir(github.join("workflows"))
        .unwrap()
        .flatten()
    {
        files.push(entry.path());
    }
    assert!(files.len() > 2);
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap();
        assert_eq!(diagnostics(&file, &text), Vec::new(), "{}", file.display());
    }
}

#[test]
fn workflow_and_dependabot_mistakes_are_found() {
    let path = Path::new("repo/.github/workflows/ci.yml");
    let text = "on: push\njobs:\n  test:\n    runs-on: ubuntu-latest\n    timeout: 5\n    steps:\n      - name: nothing\n";
    let found = diagnostics(path, text);
    let places: Vec<&str> = found.iter().map(|(at, _)| at.as_str()).collect();
    assert!(places.contains(&"timeout"), "{found:?}");
    assert!(
        found
            .iter()
            .any(|(_, m)| m.contains("uses") || m.contains("run")),
        "{found:?}"
    );
    let text = "version: 2\nupdates:\n  - package-ecosystem: cargoo\n    directory: /\n    schedule:\n      interval: weekly\n";
    let found = diagnostics(Path::new("repo/.github/dependabot.yml"), text);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].0, "cargoo");
}
