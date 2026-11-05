use super::*;
use crate::plugins::builtins::mcp::quality::{Out, Scratch, scratch};

/// A run, its report left out.
fn run_in(root: &Path, argv: &[String]) -> i32 {
    super::run_in(root, argv, &Out::default())
}

fn argv(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

fn project(src: &str) -> Scratch {
    let dir = scratch();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src/a.js"), src).unwrap();
    dir
}

const BRANCHY: &str = "export function f(x){ if(x>1 && x<5){return 1} return 2 }\n";

#[test]
fn fully_covered_crap_equals_complexity() {
    assert_eq!(crap(5, 100.0), 5.0);
}

#[test]
fn uncovered_crap_is_cc_squared_plus_cc() {
    assert_eq!(crap(3, 0.0), 12.0);
}

#[test]
fn edn_has_nil_without_coverage() {
    let e = Entry {
        name: "f".into(),
        namespace: "a".into(),
        complexity: 2,
        coverage: None,
    };
    assert_eq!(
        to_edn(&[e]),
        "{:entries [{:name \"f\" :namespace \"a\" :complexity 2 :coverage nil :crap nil}]}\n"
    );
}

#[test]
fn edn_escapes_quotes() {
    assert_eq!(edn_string("a\"b\\"), "\"a\\\"b\\\\\"");
}

#[test]
fn complexity_only_run_writes_the_snapshot() {
    let dir = project(BRANCHY);
    assert_eq!(run_in(dir.path(), &argv(&["--no-coverage"])), 0);
    let edn = std::fs::read_to_string(dir.path().join(".metrics/crap.edn")).unwrap();
    assert!(edn.contains(":name \"f\" :namespace \"a\" :complexity 3"));
}

#[test]
fn threshold_fails_a_poorly_covered_function() {
    let dir = project(BRANCHY);
    std::fs::create_dir_all(dir.path().join("coverage")).unwrap();
    std::fs::write(
        dir.path().join("coverage/lcov.info"),
        "SF:src/a.js\nDA:1,0\nend_of_record\n",
    )
    .unwrap();
    let run = |extra: &str| {
        run_in(
            dir.path(),
            &argv(&["--use-existing-coverage", "--threshold", extra]),
        )
    };
    assert_eq!(run("5"), 2);
    assert_eq!(run("50"), 0);
    let edn = std::fs::read_to_string(dir.path().join(".metrics/crap.edn")).unwrap();
    assert!(edn.contains(":coverage 0 :crap 12"));
}

#[test]
fn existing_coverage_is_required_when_asked() {
    let dir = project(BRANCHY);
    assert_eq!(run_in(dir.path(), &argv(&["--use-existing-coverage"])), 1);
}

#[test]
fn empty_tree_is_not_an_error() {
    let dir = scratch();
    assert_eq!(run_in(dir.path(), &argv(&[])), 0);
}

#[test]
fn bad_options_are_errors() {
    let dir = scratch();
    assert_eq!(run_in(dir.path(), &argv(&["--nope"])), 1);
}

#[test]
fn table_lists_worst_function_first() {
    let mk = |n: &str, c: usize| Entry {
        name: n.into(),
        namespace: "m".into(),
        complexity: c,
        coverage: Some(0.0),
    };
    let entries = [mk("low", 1), mk("high", 4)];
    let shown = worst_first(&entries);
    let text = table(&shown, &BTreeSet::from(["rust"]));
    assert!(text.find("high").unwrap() < text.find("low").unwrap());
    assert!(text.contains("Languages: rust"));
}

#[test]
fn table_layout_is_fixed() {
    let e = Entry {
        name: "f".into(),
        namespace: "m".into(),
        complexity: 1,
        coverage: Some(100.0),
    };
    assert_eq!(
        table(&[&e], &BTreeSet::from(["rust"])),
        "CRAP Report
===========
Languages: rust

Function  Namespace   CC    Cov%     CRAP
-----------------------------------------
f         m            1   100.0      1.0
"
    );
}

#[test]
fn table_columns_grow_with_long_names() {
    let e = Entry {
        name: "a_long_function_name".into(),
        namespace: "a_long_namespace_name".into(),
        complexity: 1,
        coverage: None,
    };
    let text = table(&[&e], &BTreeSet::new());
    assert!(text.contains("Function              Namespace "));
    assert!(text.contains(&"-".repeat(20 + 21 + 24)));
}

#[test]
fn edn_rounds_to_one_decimal() {
    let e = Entry {
        name: "f".into(),
        namespace: "a".into(),
        complexity: 1,
        coverage: Some(66.666),
    };
    assert!(to_edn(&[e]).contains(":coverage 66.7 :crap 1}"));
}

#[test]
fn threshold_is_exclusive() {
    let dir = project(BRANCHY);
    std::fs::create_dir_all(dir.path().join("coverage")).unwrap();
    std::fs::write(
        dir.path().join("coverage/lcov.info"),
        "SF:src/a.js
DA:1,0
end_of_record
",
    )
    .unwrap();
    let run = |t: &str| {
        run_in(
            dir.path(),
            &argv(&["--use-existing-coverage", "--threshold", t]),
        )
    };
    assert_eq!((run("12"), run("11.9")), (0, 2));
}
