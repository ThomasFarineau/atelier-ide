use super::*;
use crate::plugins::builtins::mcp::quality::{Out, Scratch, scratch};

/// A run, its report left out.
fn run_in(root: &Path, argv: &[String]) -> i32 {
    super::run_in(root, argv, &Out::default())
}

fn forms(lang: Lang, src: &str) -> Vec<Form> {
    forms_in_source(lang, src, "x")
}

fn form(start: usize, end: usize, nodes: usize) -> Form {
    Form {
        file: "x".into(),
        start,
        end,
        rust: false,
        nodes,
        prints: vec![],
    }
}

#[test]
fn renamed_locals_are_identical() {
    let f = forms(
        Lang::Ts,
        "function f(x:number){\n let a=x+1;\n let b=a*2;\n if(b>3){return 1}\n return 2\n}\nfunction g(y:number){\n let c=y+1;\n let d=c*2;\n if(d>3){return 1}\n return 2\n}",
    );
    assert_eq!(similarity(&f[0].prints, &f[1].prints), 1.0);
}

#[test]
fn different_structure_scores_low() {
    let f = forms(
        Lang::Ts,
        "function f(x){\n let a=x+1;\n let b=a*2;\n if(b>3){return 1}\n return 2\n}\nfunction g(items){\n for (const i of items) {\n  while (i.go()) { i.next(); }\n }\n return items.map(z => z.q);\n}",
    );
    assert!(similarity(&f[0].prints, &f[1].prints) < 0.5);
}

#[test]
fn small_forms_are_not_reported() {
    let f = forms(
        Lang::Js,
        "function f(x){return x+1}\nfunction g(y){return y+1}",
    );
    assert!(candidates(&f, 0.82, 4, 20, 2).is_empty());
}

fn argv(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

const FUNC: &str = "export function NAME(x){
 let a=x+1;
 let b=a*2;
 if(b>3){return 1}
 return 2
}
";

fn project(sources: &[(&str, &str)]) -> Scratch {
    let dir = scratch();
    for (name, body) in sources {
        std::fs::write(dir.path().join(name), body).unwrap();
    }
    dir
}

#[test]
fn duplicates_across_files_are_reported_and_saved() {
    let dir = project(&[
        ("a.ts", &FUNC.replace("NAME", "f")),
        ("b.ts", &FUNC.replace("NAME", "g")),
    ]);
    assert_eq!(
        run_in(dir.path(), &argv(&[])),
        2,
        "duplicates fail the gate"
    );
    let edn = std::fs::read_to_string(dir.path().join(".metrics/dry.edn")).unwrap();
    assert!(edn.contains(":score 1.00") && edn.contains("a.ts") && edn.contains("b.ts"));
}

#[test]
fn a_high_threshold_drops_near_matches() {
    let other = FUNC.replace("NAME", "g").replace("a*2", "a*2*a");
    let dir = project(&[("a.ts", &FUNC.replace("NAME", "f")), ("b.ts", &other)]);
    run_in(dir.path(), &argv(&["--threshold", "1.0"]));
    let edn = std::fs::read_to_string(dir.path().join(".metrics/dry.edn")).unwrap();
    assert_eq!(
        edn,
        "{:candidates []}
"
    );
}

#[test]
fn small_forms_can_be_allowed() {
    let dir = project(&[
        ("a.ts", "function f(x){return x+1}"),
        ("b.ts", "function g(y){return y+1}"),
    ]);
    run_in(dir.path(), &argv(&["--min-lines", "1", "--min-nodes", "1"]));
    let edn = std::fs::read_to_string(dir.path().join(".metrics/dry.edn")).unwrap();
    assert!(edn.contains(":score 1.00"));
}

#[test]
fn rust_and_script_forms_never_match_each_other() {
    let rust = "fn f(x: i32) -> i32 {
 let a = x + 1;
 let b = a * 2;
 if b > 3 { return 1; }
 2
}
";
    let dir = project(&[("a.ts", &FUNC.replace("NAME", "f")), ("b.rs", rust)]);
    run_in(
        dir.path(),
        &argv(&["--threshold", "0.1", "--min-nodes", "1"]),
    );
    let edn = std::fs::read_to_string(dir.path().join(".metrics/dry.edn")).unwrap();
    assert_eq!(
        edn,
        "{:candidates []}
"
    );
}

#[test]
fn empty_tree_and_bad_options() {
    let dir = scratch();
    assert_eq!(run_in(dir.path(), &argv(&[])), 0);
    assert_eq!(run_in(dir.path(), &argv(&["--threshold", "x"])), 1);
}

#[test]
fn similarity_counts_shared_over_union() {
    assert_eq!(similarity(&[1, 2, 3, 4], &[2, 3, 5]), 0.4);
    assert_eq!(similarity(&[1, 2, 3], &[1, 2, 3]), 1.0);
    assert_eq!(similarity(&[5], &[5]), 1.0);
    assert_eq!(similarity(&[], &[]), 0.0);
    assert_eq!(similarity(&[1, 2, 3], &[1]), 1.0 / 3.0);
    assert_eq!(similarity(&[1], &[1, 2, 3]), 1.0 / 3.0);
    assert_eq!(similarity(&[1, 2], &[3, 4]), 0.0);
}

#[test]
fn nodes_are_counted() {
    let tree = parse(Lang::Js, "function f(){}").unwrap();
    let f = &funcs::functions(&tree, "function f(){}", Lang::Js, "")[0];
    assert_eq!(fingerprint(f.node, "function f(){}").1, 4);
}

#[test]
fn size_limits_are_inclusive() {
    let f = form(1, 4, 20);
    assert!(f.is_big_enough(4, 20));
    assert!(!f.is_big_enough(5, 20));
    assert!(!f.is_big_enough(4, 21));
}

#[test]
fn only_big_enough_forms_are_compared() {
    let (mut a, mut b) = (form(1, 4, 20), form(10, 13, 20));
    (a.prints, b.prints) = (vec![1, 2], vec![1, 2]);
    assert_eq!(candidates(&[a, b], 0.5, 4, 20, 2).len(), 1);
    let (mut a, mut b) = (form(1, 4, 20), form(10, 13, 20));
    (a.prints, b.prints) = (vec![1, 2], vec![1, 2]);
    assert!(candidates(&[a, b], 0.5, 5, 20, 2).is_empty());
}

#[test]
fn comments_do_not_change_the_structure() {
    let plain = forms(
        Lang::Ts,
        "function f(x){
 let a=x+1;
 return a;
}",
    );
    let noisy = forms(
        Lang::Ts,
        "function f(x){
 // note
 let a=x+1; /* more */
 return a;
}",
    );
    assert_eq!(plain[0].prints, noisy[0].prints);
    let rust = forms(
        Lang::Rust,
        "fn f(x: i32) -> i32 {
 // note
 x + 1
}",
    );
    assert_eq!(
        rust[0].prints,
        forms(
            Lang::Rust,
            "fn f(x: i32) -> i32 {
 x + 1
}"
        )[0]
        .prints
    );
}

#[test]
fn called_names_matter_but_other_names_do_not() {
    let base = forms(Lang::Js, "function f(a){ return go(a.x); }");
    let renamed = forms(Lang::Js, "function g(b){ return go(b.y); }");
    let other_call = forms(Lang::Js, "function f(a){ return stop(a.x); }");
    let other_method = forms(Lang::Js, "function f(a){ return a.run(); }");
    let method = forms(Lang::Js, "function f(a){ return a.walk(); }");
    assert_eq!(base[0].prints, renamed[0].prints);
    assert_ne!(base[0].prints, other_call[0].prints);
    assert_ne!(other_method[0].prints, method[0].prints);
}

#[test]
fn options_have_defaults() {
    let args = Args::parse(&[], &[], &["threshold", "min-lines", "min-nodes", "jobs"]).unwrap();
    let o = options(&args).unwrap();
    assert_eq!(
        (o.threshold, o.min_lines, o.min_nodes, o.jobs),
        (0.8, 4, 20, par::default_jobs())
    );
}

#[test]
fn a_score_equal_to_the_threshold_is_reported() {
    let (mut a, mut b) = (form(1, 4, 20), form(10, 13, 20));
    (a.prints, b.prints) = (vec![1, 2], vec![1, 2]);
    assert_eq!(candidates(&[a, b], 1.0, 4, 20, 2).len(), 1);
}
