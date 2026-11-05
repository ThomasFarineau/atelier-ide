use super::*;
use crate::plugins::builtins::mcp::quality::{Out, Scratch, scratch};

/// A run, its report left out.
fn run_in(root: &Path, argv: &[String]) -> i32 {
    let _serial = crate::plugins::builtins::mcp::quality::run::serial();
    super::run_in(root, argv, &Out::default())
}

fn found(lang: Lang, src: &str) -> Vec<String> {
    let tree = parse(lang, src).unwrap();
    sites_in(tree.root_node(), src)
        .iter()
        .map(|s| format!("{}->{}", s.from, s.to))
        .collect()
}

fn argv(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

const SRC: &str = "export function f(x){ return x > 1 }\n";
/// Passes on the original source, fails once `>` or `1` is mutated.
const GUARD: &str = if cfg!(windows) {
    "findstr /C:\"x > 1\" src\\a.js"
} else {
    "grep -q 'x > 1' src/a.js"
};

fn project(src: &str) -> Scratch {
    let dir = scratch();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src/a.js"), src).unwrap();
    dir
}

fn mutate(dir: &Scratch, extra: &[&str], cmd: &str) -> i32 {
    let mut a = vec!["--no-coverage", "--test-command", cmd];
    a.extend_from_slice(extra);
    run_in(dir.path(), &argv(&a))
}

fn snapshot(dir: &Scratch) -> String {
    std::fs::read_to_string(dir.path().join(".metrics/mutate/a.edn")).unwrap()
}

#[test]
fn script_sites_cover_operators_and_literals() {
    assert_eq!(
        found(Lang::Js, "f(a > 1 && true)"),
        vec!["&&->||", ">->>=", "1->0", "true->false"]
    );
}

#[test]
fn rust_sites_cover_operators_and_literals() {
    assert_eq!(
        found(Lang::Rust, "fn f(a: i32) -> bool { a + 0 == 2 }"),
        vec!["==->!=", "+->-", "0->1", "2->0"]
    );
    assert_eq!(
        found(Lang::Rust, "fn f() -> bool { true }"),
        vec!["true->false"]
    );
}

#[test]
fn type_level_literals_are_not_mutated() {
    assert_eq!(found(Lang::Ts, "type A = 1;\nlet x: 2 = 2;"), vec!["2->0"]);
}

#[test]
fn snapshot_fields_round_trip() {
    let line = "  {:key \"f#0\" :name \"f\" :hash \"ab\" :killed 3 :survived 1 :uncovered 2 :survivors []}";
    assert_eq!(str_field(line, ":hash"), Some("ab"));
    let (key, p) = parse_previous(line).unwrap();
    assert_eq!((key.as_str(), p.hash.as_str()), ("f#0", "ab"));
    assert_eq!(
        p.counts,
        Counts {
            killed: 3,
            survived: 1,
            uncovered: 2
        }
    );
    assert!(parse_previous("{:name \"x\"}").is_none());
}

#[test]
fn score_is_killed_over_executed() {
    assert_eq!(
        Counts {
            killed: 3,
            survived: 1,
            uncovered: 9
        }
        .score(),
        75.0
    );
    assert_eq!(Counts::default().score(), 100.0);
}

#[test]
fn scan_lists_sites_without_running_tests() {
    let dir = project(SRC);
    assert_eq!(run_in(dir.path(), &argv(&["--scan"])), 0);
    assert!(!dir.path().join(".metrics").exists());
}

#[test]
fn killed_mutants_exit_zero_and_leave_the_source_alone() {
    let dir = project(SRC);
    assert_eq!(mutate(&dir, &[], GUARD), 0);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("src/a.js")).unwrap(),
        SRC
    );
    assert!(snapshot(&dir).contains(":killed 2 :survived 0 :uncovered 0 :survivors []"));
}

#[test]
fn surviving_mutants_exit_three_and_are_listed() {
    let dir = project(SRC);
    assert_eq!(mutate(&dir, &[], "exit 0"), 3);
    let snap = snapshot(&dir);
    assert!(snap.contains(":killed 0 :survived 2") && snap.contains("src/a.js:1 > -> >= [f]"));
}

#[test]
fn failing_baseline_exits_two_and_touches_nothing() {
    let dir = project(SRC);
    assert_eq!(mutate(&dir, &[], "exit 1"), 2);
    assert!(!dir.path().join(".metrics").exists());
}

#[test]
fn unchanged_clean_functions_are_not_rerun_unless_asked() {
    let dir = project(SRC);
    assert_eq!(mutate(&dir, &[], GUARD), 0);
    assert_eq!(mutate(&dir, &[], "exit 0"), 0);
    assert_eq!(mutate(&dir, &["--mutate-all"], "exit 0"), 3);
}

#[test]
fn edited_functions_are_rerun() {
    let dir = project(SRC);
    assert_eq!(mutate(&dir, &[], GUARD), 0);
    std::fs::write(
        dir.path().join("src/a.js"),
        SRC.replace("return", "return !!"),
    )
    .unwrap();
    assert_eq!(mutate(&dir, &[], "exit 0"), 3);
}

#[test]
fn uncovered_sites_are_counted_not_run() {
    let dir = project(SRC);
    std::fs::create_dir_all(dir.path().join("coverage")).unwrap();
    std::fs::write(
        dir.path().join("coverage/lcov.info"),
        "SF:src/a.js\nDA:1,0\nend_of_record\n",
    )
    .unwrap();
    let code = run_in(
        dir.path(),
        &argv(&["--use-existing-coverage", "--test-command", "exit 0"]),
    );
    assert_eq!(code, 0);
    assert!(snapshot(&dir).contains(":killed 0 :survived 0 :uncovered 2"));
}

#[test]
fn files_in_coverage_with_hits_are_mutated() {
    let dir = project(SRC);
    std::fs::create_dir_all(dir.path().join("coverage")).unwrap();
    std::fs::write(
        dir.path().join("coverage/lcov.info"),
        "SF:src/a.js\nDA:1,4\nend_of_record\n",
    )
    .unwrap();
    let code = run_in(
        dir.path(),
        &argv(&["--use-existing-coverage", "--test-command", GUARD]),
    );
    assert_eq!(code, 0);
    assert!(snapshot(&dir).contains(":killed 2"));
}

fn suite_for(items: &[&str], root: &Path, lang: Lang) -> Result<Suite, String> {
    suite(
        &Args::parse(&argv(items), &[], &["test-command", "focus-command"]).unwrap(),
        root,
        lang,
    )
}

#[test]
fn rust_libraries_test_the_module_then_the_library() {
    let dir = scratch();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::write(dir.path().join("src/lib.rs"), "").unwrap();
    let s = suite_for(&[], dir.path(), Lang::Rust).unwrap();
    assert_eq!(
        s.focus.as_deref(),
        Some("cargo test --lib --quiet -- {module}::")
    );
    assert_eq!(s.full, "cargo test --lib --quiet");
}

#[test]
fn rust_crates_without_a_library_test_every_target() {
    let dir = scratch();
    let s = suite_for(&[], dir.path(), Lang::Rust).unwrap();
    assert_eq!(s.full, "cargo test --quiet");
}

#[test]
fn custom_commands_replace_the_defaults() {
    let dir = scratch();
    assert!(suite_for(&[], dir.path(), Lang::Js).is_err());
    let s = suite_for(
        &["--test-command", "x", "--focus-command", "y {file}"],
        dir.path(),
        Lang::Js,
    )
    .unwrap();
    assert_eq!(
        s,
        Suite {
            focus: Some("y {file}".into()),
            full: "x".into()
        }
    );
}

#[test]
fn focus_filters_on_the_module_inside_its_crate() {
    assert_eq!(focus_module("ubcore::a::b", Lang::Rust), "a::b");
    assert_eq!(focus_module("ubcore", Lang::Rust), "");
    assert_eq!(focus_module("a.b", Lang::Js), "a.b");
}

#[test]
fn a_mutant_faces_its_module_tests_then_the_full_suite() {
    let s = Suite {
        focus: Some("f {module} {file}".into()),
        full: "t".into(),
    };
    assert_eq!(steps(&s, "m", "src/m.js"), vec!["f m src/m.js", "t"]);
    assert_eq!(steps(&s, "", "src/lib.rs"), vec!["t"]);
    assert_eq!(
        steps(
            &Suite {
                focus: None,
                full: "t".into()
            },
            "m",
            "src/m.js"
        ),
        vec!["t"]
    );
}

#[test]
fn the_first_step_that_does_not_pass_decides() {
    let steps: Vec<String> = ["a", "b", "c"].map(String::from).to_vec();
    let ran = Mutex::new(Vec::new());
    let run = |answer: &'static [(&str, Outcome)]| {
        ran.lock().unwrap().clear();
        first_failure(&steps, |c| {
            ran.lock().unwrap().push(c.to_string());
            answer
                .iter()
                .find(|(n, _)| *n == c)
                .map_or(Outcome::Pass, |(_, o)| *o)
        })
    };
    assert_eq!(run(&[("b", Outcome::Fail)]), Outcome::Fail);
    assert_eq!(*ran.lock().unwrap(), vec!["a", "b"]);
    assert_eq!(run(&[("a", Outcome::Timeout)]), Outcome::Timeout);
    assert_eq!(run(&[]), Outcome::Pass);
    assert_eq!(ran.lock().unwrap().len(), 3);
}

/// Passes while `x > 1` is intact in the file of module `{module}`.
const FOCUS_GUARD: &str = if cfg!(windows) {
    "findstr /C:\"x > 1\" src\\{module}.js"
} else {
    "grep -q 'x > 1' src/{module}.js"
};

#[test]
fn focused_tests_kill_mutants_without_the_full_suite() {
    let dir = project(SRC);
    assert_eq!(mutate(&dir, &["--focus-command", FOCUS_GUARD], "exit 0"), 0);
    assert!(snapshot(&dir).contains(":killed 2 :survived 0"));
}

#[test]
fn survivors_of_the_focused_tests_face_the_full_suite() {
    let dir = project(SRC);
    assert_eq!(mutate(&dir, &["--focus-command", "exit 0"], GUARD), 0);
    assert!(snapshot(&dir).contains(":killed 2 :survived 0"));
}

#[test]
fn focused_tests_failing_on_the_original_stop_the_run() {
    let dir = project(SRC);
    assert_eq!(mutate(&dir, &["--focus-command", "exit 1"], "exit 0"), 2);
    assert!(!dir.path().join(".metrics").exists());
}

#[test]
fn empty_tree_and_bad_options() {
    let dir = scratch();
    assert_eq!(run_in(dir.path(), &argv(&[])), 0);
    assert_eq!(run_in(dir.path(), &argv(&["--nope"])), 1);
    assert_eq!(run_in(dir.path(), &argv(&["--jobs", "0"])), 1);
}

#[test]
fn mutant_timeouts_scale_with_the_baseline() {
    let t = |s: u64| mutant_timeout(Duration::from_secs(s)).as_secs();
    assert_eq!((t(0), t(1), t(2), t(10)), (10, 10, 11, 35));
}

#[test]
fn missing_snapshot_numbers_are_zero() {
    assert_eq!(num_field("{}", ":killed"), 0);
}

#[test]
fn a_single_survivor_fails_the_run() {
    let dir = project("export function f(x, y){ return x > y }\n");
    assert_eq!(mutate(&dir, &[], "exit 0"), 3);
}

#[test]
fn rust_files_use_their_own_test_command() {
    let dir = scratch();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    let src = "pub fn f(x: i32) -> bool { x > 1 }\n";
    std::fs::write(dir.path().join("src/a.rs"), src).unwrap();
    let guard = if cfg!(windows) {
        "findstr /C:\"x > 1\" src\\a.rs"
    } else {
        "grep -q 'x > 1' src/a.rs"
    };
    assert_eq!(mutate(&dir, &[], guard), 0);
    assert!(snapshot(&dir).contains(":killed 2"));
    assert_eq!(
        std::fs::read_to_string(dir.path().join("src/a.rs")).unwrap(),
        src
    );
}

const MANY: &str = "export function f(x){ return x > 1 }\nexport function g(x){ return x > 2 }\nexport function h(x){ return x > 3 }\n";

/// Passes only while all three functions of `MANY` are intact.
fn guard_many() -> String {
    let one = |n: u32| {
        if cfg!(windows) {
            format!("findstr /C:\"x > {n}\" src\\a.js")
        } else {
            format!("grep -q 'x > {n}' src/a.js")
        }
    };
    format!("{} && {} && {}", one(1), one(2), one(3))
}

#[test]
fn any_number_of_workers_gives_the_same_result() {
    for jobs in ["1", "2", "6"] {
        let dir = project(MANY);
        assert_eq!(
            mutate(&dir, &["--jobs", jobs], &guard_many()),
            0,
            "jobs {jobs}"
        );
        assert_eq!(
            snapshot(&dir).matches(":killed 2 :survived 0").count(),
            3,
            "jobs {jobs}"
        );
        let dir = project(MANY);
        assert_eq!(mutate(&dir, &["--jobs", jobs], "exit 0"), 3, "jobs {jobs}");
        assert_eq!(
            snapshot(&dir).matches(":survived 2").count(),
            3,
            "jobs {jobs}"
        );
    }
}

#[test]
fn workers_work_on_copies_and_are_kept_unless_cleaned() {
    let dir = project(SRC);
    assert_eq!(mutate(&dir, &["--jobs", "2"], GUARD), 0);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("src/a.js")).unwrap(),
        SRC
    );
    assert!(
        dir.path()
            .join("target/mutation-workers/w0/src/a.js")
            .is_file()
    );
    assert_eq!(
        mutate(
            &dir,
            &["--jobs", "2", "--mutate-all", "--clean-workers"],
            GUARD
        ),
        0
    );
    assert!(!dir.path().join("target/mutation-workers").exists());
}

#[test]
fn every_worker_checks_the_baseline() {
    let dir = project(MANY);
    assert_eq!(mutate(&dir, &["--jobs", "3"], "exit 1"), 2);
}
