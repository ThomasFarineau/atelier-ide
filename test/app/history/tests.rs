use super::*;

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("atelier-history-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn a_version_is_kept_once_and_the_latest_comes_first() {
    let base = temp("keep");
    let (root, file) = (Path::new("/p"), Path::new("/p/a.rs"));
    record(&base, root, file, "one\n");
    record(&base, root, file, "one\n");
    record(&base, root, file, "one\ntwo\n");
    let versions = list(&base, root, file);
    assert_eq!(versions.len(), 2);
    assert_eq!(fs::read_to_string(&versions[0].path).unwrap(), "one\ntwo\n");
    assert_eq!(fs::read_to_string(&versions[1].path).unwrap(), "one\n");
    // Another file has its own versions.
    assert!(list(&base, root, Path::new("/p/b.rs")).is_empty());
    let _ = fs::remove_dir_all(base);
}

#[test]
fn the_oldest_versions_go_first() {
    let base = temp("prune");
    let (root, file) = (Path::new("/p"), Path::new("/p/a.rs"));
    for i in 0..(MAX_VERSIONS + 5) {
        record(&base, root, file, &format!("version {i}\n"));
    }
    let versions = list(&base, root, file);
    assert_eq!(versions.len(), MAX_VERSIONS);
    assert_eq!(
        fs::read_to_string(&versions[0].path).unwrap(),
        format!("version {}\n", MAX_VERSIONS + 4)
    );
    let _ = fs::remove_dir_all(base);
}

#[test]
fn changes_count_the_lines() {
    assert_eq!(changes("a\nb\nc\n", "a\nx\nc\nd\n"), (2, 1));
}

#[test]
fn a_comparison_keeps_the_changes_and_a_few_lines_around() {
    let old: String = (1..=20).map(|i| format!("line {i}\n")).collect();
    let new = old.replace("line 10\n", "ten\n");
    let rows = compare(&old, &new);
    assert!(rows.contains(&Row::Removed(10, "line 10".into())));
    assert!(rows.contains(&Row::Added(10, "ten".into())));
    assert!(rows.contains(&Row::Same(7, "line 7".into())));
    assert!(!rows.iter().any(|r| matches!(r, Row::Same(6, _))));
    assert!(compare(&old, &old).is_empty());
}
