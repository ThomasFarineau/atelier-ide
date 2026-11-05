use super::*;
use crate::plugins::builtins::mcp::quality::{Out, Scratch, scratch};

/// A run, its report left out.
fn run_in(root: &Path, argv: &[String]) -> i32 {
    super::run_in(root, argv, &Out::default())
}

fn def(name: &str, start: usize, end: usize, crap: f64) -> Def {
    Def {
        name: name.into(),
        file: "a.rs".into(),
        start,
        end,
        crap,
        exits_early: false,
    }
}

fn call(name: &str, line: usize) -> Use {
    Use {
        name: name.into(),
        file: "a.rs".into(),
        line,
        call: true,
        in_closure: false,
    }
}

fn found(defs: &[Def], uses: &[Use], limit: f64) -> Vec<(String, String, usize)> {
    merges(defs, uses, limit)
        .iter()
        .map(|m| (m.function.name.clone(), m.into.name.clone(), m.line))
        .collect()
}

#[test]
fn a_function_called_once_merges_into_its_caller_under_the_threshold() {
    let defs = [def("helper", 1, 3, 2.0), def("caller", 5, 9, 3.0)];
    assert_eq!(
        found(&defs, &[call("helper", 7)], 8.0),
        vec![("helper".into(), "caller".into(), 7)]
    );
    assert!(
        found(&defs, &[call("helper", 7)], 5.0).is_empty(),
        "2 + 3 is not under 5"
    );
}

#[test]
fn the_innermost_function_around_the_call_is_the_caller() {
    let defs = [
        def("helper", 1, 2, 1.0),
        def("outer", 3, 20, 1.0),
        def("inner", 15, 19, 1.0),
    ];
    assert_eq!(
        found(&defs, &[call("helper", 16)], 8.0),
        vec![("helper".into(), "inner".into(), 16)]
    );
}

#[test]
fn functions_used_twice_or_defined_twice_stay() {
    let defs = [def("helper", 1, 3, 1.0), def("caller", 5, 9, 1.0)];
    assert!(found(&defs, &[call("helper", 6), call("helper", 7)], 8.0).is_empty());
    let twice = [
        def("helper", 1, 3, 1.0),
        def("caller", 5, 9, 1.0),
        def("helper", 11, 12, 1.0),
    ];
    assert!(found(&twice, &[call("helper", 6)], 8.0).is_empty());
}

#[test]
fn only_a_call_from_another_function_counts() {
    let defs = [def("helper", 1, 3, 1.0), def("caller", 5, 9, 1.0)];
    let value = Use {
        call: false,
        ..call("helper", 6)
    };
    assert!(found(&defs, &[value], 8.0).is_empty(), "passed as a value");
    assert!(
        found(&defs, &[call("helper", 2)], 8.0).is_empty(),
        "recursive"
    );
    assert!(
        found(&defs, &[call("helper", 30)], 8.0).is_empty(),
        "called from no function, like a test"
    );
    let elsewhere = Use {
        file: "b.rs".into(),
        ..call("helper", 6)
    };
    assert!(
        found(&defs, &[elsewhere], 8.0).is_empty(),
        "same lines, other file"
    );
}

#[test]
fn calls_from_a_closure_and_functions_that_exit_early_stay() {
    let defs = [def("helper", 1, 3, 1.0), def("caller", 5, 9, 1.0)];
    let from_closure = Use {
        in_closure: true,
        ..call("helper", 6)
    };
    assert!(
        found(&defs, &[from_closure], 8.0).is_empty(),
        "the body would land in a closure"
    );
    let early = [
        Def {
            exits_early: true,
            ..def("helper", 1, 3, 1.0)
        },
        def("caller", 5, 9, 1.0),
    ];
    assert!(
        found(&early, &[call("helper", 6)], 8.0).is_empty(),
        "its exits would leave the caller"
    );
}

fn exits(file: &str, src: &str) -> Vec<(String, bool)> {
    scanned(file, src)
        .0
        .iter()
        .map(|d| (d.name.clone(), d.exits_early))
        .collect()
}

fn pairs(items: &[(&str, bool)]) -> Vec<(String, bool)> {
    items.iter().map(|(n, b)| (n.to_string(), *b)).collect()
}

#[test]
fn rust_early_exits_are_question_marks_let_else_and_returns_before_the_end() {
    let src = "fn q() -> Option<i32> { let x = g()?; Some(x) }\n\
               fn r(a: bool) -> i32 { if a { return 1; } 2 }\n\
               fn l(o: Option<i32>) -> i32 { let Some(x) = o else { return 0 }; x }\n\
               fn last() -> i32 { return 1; }\n\
               fn inner() -> i32 { let f = |o: Option<i32>| -> Option<i32> { o? ; return o }; 1 }\n\
               fn plain() -> i32 { 1 }\n\
               fn mid() -> i32 { return 1; 2 }\n\
               fn panics(o: Option<i32>) -> i32 { let Some(x) = o else { panic!() }; x }\n\
               fn branch(a: bool) -> i32 { if a { 1 } else { 2 } }\n";
    let want = [
        ("q", true),
        ("r", true),
        ("l", true),
        ("last", false),
        ("inner", false),
        ("plain", false),
        ("mid", true),
        ("panics", true),
        ("branch", false),
    ];
    assert_eq!(exits("a.rs", src), pairs(&want));
}

#[test]
fn script_early_exits_are_returns_before_the_end() {
    let src = "function e(x){ if (x) return 1; return 2 }\n\
               function f(){ return 1 }\n\
               const g = () => 1\n\
               function h(){ [1].map(x => { return x }); return 2 }\n";
    assert_eq!(
        exits("a.js", src),
        pairs(&[("e", true), ("f", false), ("g", false), ("h", false)])
    );
}

fn closures(file: &str, src: &str, name: &str) -> Vec<(usize, bool)> {
    scanned(file, src)
        .1
        .iter()
        .filter(|u| u.name == name)
        .map(|u| (u.line, u.in_closure))
        .collect()
}

#[test]
fn calls_inside_closures_are_marked() {
    let rust = "fn a() { h(); }\nfn b() { v.iter().map(|x| h(x)); }\nfn c() { fn d() { h() } }\n";
    assert_eq!(
        closures("a.rs", rust, "h"),
        vec![(1, false), (2, true), (3, false)]
    );
    let js = "const a = () => h()\n\
              function b(){ [1].map(x => h(x)) }\n\
              function c(){ [1].map(function(x){ return h(x) }) }\n\
              class K { m(){ return h() } }\n";
    assert_eq!(
        closures("a.js", js, "h"),
        vec![(1, false), (2, true), (3, true), (4, false)]
    );
}

fn scanned(file: &str, src: &str) -> (Vec<Def>, Vec<Use>) {
    let dir = scratch();
    let path = dir.path().join(file);
    std::fs::write(&path, src).unwrap();
    scan(dir.path(), &path, None)
}

fn mentions(uses: &[Use], name: &str) -> Vec<(usize, bool)> {
    uses.iter()
        .filter(|u| u.name == name)
        .map(|u| (u.line, u.call))
        .collect()
}

#[test]
fn rust_calls_in_every_form_are_found() {
    let src = "fn h() -> i32 { 1 }\nfn a() -> i32 { h() }\nfn b() -> i32 { m::h() }\nfn c(x: S) -> i32 { x.h() }\nfn d() -> i32 { h::<u8>() }\n";
    let (defs, uses) = scanned("a.rs", src);
    assert_eq!(
        defs.iter()
            .map(|d| (d.name.as_str(), d.start, d.crap))
            .collect::<Vec<_>>(),
        vec![
            ("h", 1, 1.0),
            ("a", 2, 1.0),
            ("b", 3, 1.0),
            ("c", 4, 1.0),
            ("d", 5, 1.0)
        ]
    );
    assert_eq!(
        mentions(&uses, "h"),
        vec![(2, true), (3, true), (4, true), (5, true)]
    );
}

#[test]
fn rust_values_macros_and_bindings() {
    let src =
        "fn h() {}\nfn a() { let f = h; println!(\"{}\", h()); let h = 2; x.h; h.len(); m::h; }\n";
    let (_, uses) = scanned("a.rs", src);
    let none = vec![(2, false); 5];
    assert_eq!(
        mentions(&uses, "h"),
        none,
        "value, macro argument, field, receiver, path"
    );
}

#[test]
fn script_calls_values_and_methods_are_found() {
    let src = "function h(x){ return x }\nfunction a(){ return h(1) }\nconst b = () => [1].map(h)\nclass C { m(){ return this.h() } }\n";
    let (defs, uses) = scanned("a.js", src);
    assert!(defs.iter().any(|d| d.name == "b"));
    assert_eq!(mentions(&uses, "h"), vec![(2, true), (3, false), (4, true)]);
}

fn project(files: &[(&str, &str)]) -> Scratch {
    let dir = scratch();
    for (name, body) in files {
        let p = dir.path().join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }
    dir
}

fn argv(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

const PAIR: &str =
    "function helper(x){ return x + 1 }\nexport function caller(x){ return helper(x) * 2 }\n";

#[test]
fn proposed_merges_exit_two_and_are_written() {
    let dir = project(&[("src/a.js", PAIR)]);
    assert_eq!(run_in(dir.path(), &argv(&["--no-coverage"])), 2);
    let edn = std::fs::read_to_string(dir.path().join(".metrics/merge.edn")).unwrap();
    assert!(
        edn.contains(
            ":function \"helper\" :into \"caller\" :file \"src/a.js\" :line 2 :crap 1 :into-crap 1"
        ),
        "{edn}"
    );
    assert_eq!(
        run_in(dir.path(), &argv(&["--no-coverage", "--threshold", "2"])),
        0,
        "1 + 1 is not under 2"
    );
}

#[test]
fn calls_from_test_files_count_even_when_filtered_out() {
    let dir = project(&[
        ("src/a.js", PAIR),
        (
            "tests/a.test.js",
            "import { helper } from '../src/a'\nhelper(3)\n",
        ),
    ]);
    assert_eq!(run_in(dir.path(), &argv(&["--no-coverage"])), 0);
    let dir = project(&[
        ("src/a.js", PAIR),
        ("src/b.js", "export function other(){ return helper(2) }\n"),
    ]);
    assert_eq!(run_in(dir.path(), &argv(&["--no-coverage", "src/a.js"])), 0);
}

#[test]
fn filters_choose_what_is_reported() {
    let dir = project(&[
        ("src/a.js", PAIR),
        ("src/b.js", "export function z(){ return 1 }\n"),
    ]);
    assert_eq!(run_in(dir.path(), &argv(&["--no-coverage", "src/b.js"])), 0);
    assert_eq!(run_in(dir.path(), &argv(&["--no-coverage", "src/a.js"])), 2);
}

#[test]
fn coverage_raises_the_crap() {
    let dir = project(&[
        ("src/a.js", PAIR),
        (
            "coverage/lcov.info",
            "SF:src/a.js\nDA:1,0\nDA:2,0\nend_of_record\n",
        ),
    ]);
    assert_eq!(
        run_in(dir.path(), &argv(&["--use-existing-coverage"])),
        2,
        "1x1 + 1 = 2 each, 4 < 8"
    );
    assert_eq!(
        run_in(
            dir.path(),
            &argv(&["--use-existing-coverage", "--threshold", "4"])
        ),
        0
    );
}

#[test]
fn empty_tree_and_bad_options() {
    let dir = scratch();
    assert_eq!(run_in(dir.path(), &argv(&[])), 0);
    assert_eq!(run_in(dir.path(), &argv(&["--nope"])), 1);
    assert_eq!(run_in(dir.path(), &argv(&["--threshold", "x"])), 1);
}
