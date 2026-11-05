use super::*;
use crate::plugins::builtins::mcp::quality::{Scratch, scratch};
use std::process::Command;

fn tree(files: &[&str]) -> Scratch {
    let dir = scratch();
    for f in files {
        let p = dir.path().join(f);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, "x").unwrap();
    }
    dir
}

fn names(root: &Path, files: Vec<PathBuf>) -> Vec<String> {
    files.iter().map(|f| rel(root, f)).collect()
}

fn select_names(dir: &Scratch, args: &[&str]) -> Vec<String> {
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    names(dir.path(), select(dir.path(), &args, false).unwrap())
}

#[test]
fn walks_sources_and_skips_tests_and_vendored_dirs() {
    let dir = tree(&[
        "src/a.ts",
        "src/a.test.ts",
        "src/b.d.ts",
        "src/c.md",
        "node_modules/x/i.js",
        "tests/t.rs",
        "target/g.rs",
        ".hidden/h.js",
        "lib/b.js",
        "spec/s.js",
        "foo_test.rs",
    ]);
    assert_eq!(select_names(&dir, &[]), vec!["lib/b.js", "src/a.ts"]);
}

#[test]
fn with_tests_adds_test_files_and_dirs_but_not_vendored_ones() {
    let dir = tree(&[
        "src/a.ts",
        "src/a.test.ts",
        "src/b.d.ts",
        "src/c.md",
        "node_modules/x/i.js",
        "tests/t.rs",
        "target/g.rs",
        ".hidden/h.js",
        "spec/s.js",
        "foo_test.rs",
    ]);
    let found = names(dir.path(), with_tests(dir.path()));
    assert_eq!(
        found,
        vec![
            "foo_test.rs",
            "spec/s.js",
            "src/a.test.ts",
            "src/a.ts",
            "tests/t.rs"
        ]
    );
}

#[test]
fn arguments_may_be_files_directories_or_fragments() {
    let dir = tree(&["src/a.ts", "src/deep/b.ts", "lib/c.js"]);
    assert_eq!(select_names(&dir, &["src/a.ts"]), vec!["src/a.ts"]);
    assert_eq!(
        select_names(&dir, &["src"]),
        vec!["src/a.ts", "src/deep/b.ts"]
    );
    assert_eq!(select_names(&dir, &["deep"]), vec!["src/deep/b.ts"]);
    assert_eq!(
        select_names(&dir, &["src", "src/a.ts"]),
        vec!["src/a.ts", "src/deep/b.ts"]
    );
}

#[test]
fn changed_lists_git_additions_and_edits() {
    let dir = tree(&[
        "src/old.ts",
        "src/new.ts",
        "src/new.test.ts",
        "node_modules/n.js",
    ]);
    let git = |args: &[&str]| {
        assert!(
            Command::new("git")
                .args(args)
                .current_dir(dir.path())
                .output()
                .unwrap()
                .status
                .success()
        );
    };
    git(&["init", "-q"]);
    git(&["add", "src/old.ts"]);
    git(&[
        "-c",
        "user.name=t",
        "-c",
        "user.email=t@t",
        "-c",
        "commit.gpgsign=false",
        "commit",
        "-qm",
        "x",
    ]);
    std::fs::write(dir.path().join("src/old.ts"), "edited").unwrap();
    let found = select(dir.path(), &[], true).unwrap();
    assert_eq!(names(dir.path(), found), vec!["src/new.ts", "src/old.ts"]);
}
