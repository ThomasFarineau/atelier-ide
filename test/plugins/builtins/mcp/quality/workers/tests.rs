use super::*;
use crate::plugins::builtins::mcp::quality::{Scratch, scratch};

fn project() -> Scratch {
    let dir = scratch();
    for (name, body) in [
        ("src/a.js", "a"),
        ("package.json", "{}"),
        ("target/skip.txt", "x"),
        (".git/skip", "x"),
    ] {
        let p = dir.path().join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }
    dir
}

#[test]
fn workers_get_the_sources_but_not_build_output_or_git() {
    let root = project();
    let w = sync(root.path(), 0).unwrap();
    assert_eq!(std::fs::read_to_string(w.join("src/a.js")).unwrap(), "a");
    assert!(w.join("package.json").is_file());
    assert!(!w.join("target/skip.txt").exists() && !w.join(".git").exists());
}

#[test]
fn only_changed_files_are_copied_again() {
    let root = project();
    let w = sync(root.path(), 1).unwrap();
    let untouched = std::fs::metadata(w.join("package.json"))
        .unwrap()
        .modified()
        .unwrap();
    std::fs::write(w.join("cache.txt"), "kept").unwrap();
    std::fs::write(root.path().join("src/a.js"), "changed").unwrap();
    sync(root.path(), 1).unwrap();
    assert_eq!(
        std::fs::read_to_string(w.join("src/a.js")).unwrap(),
        "changed"
    );
    assert_eq!(
        std::fs::metadata(w.join("package.json"))
            .unwrap()
            .modified()
            .unwrap(),
        untouched
    );
    assert_eq!(
        std::fs::read_to_string(w.join("cache.txt")).unwrap(),
        "kept"
    );
}

#[test]
fn node_modules_is_linked_and_survives_cleaning() {
    let root = project();
    std::fs::create_dir_all(root.path().join("node_modules/pkg")).unwrap();
    std::fs::write(root.path().join("node_modules/pkg/i.js"), "dep").unwrap();
    let w = sync(root.path(), 0).unwrap();
    assert_eq!(
        std::fs::read_to_string(w.join("node_modules/pkg/i.js")).unwrap(),
        "dep"
    );
    clean(root.path());
    assert!(!root.path().join("target/mutation-workers").exists());
    assert_eq!(
        std::fs::read_to_string(root.path().join("node_modules/pkg/i.js")).unwrap(),
        "dep"
    );
}

#[test]
fn worker_directories_are_numbered() {
    assert!(worker_dir(Path::new("/p"), 3).ends_with("target/mutation-workers/w3"));
}
