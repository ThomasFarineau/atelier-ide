use super::*;
use crate::graph::{build::build, extract::extract};
use std::{collections::BTreeMap, path::Path};

fn graph() -> Graph {
    // One folder per call: the tests run side by side and each removes its own.
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("atelier-graph-query-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    let sources = [
        (
            "src/math.js",
            "export function addNumbers(a, b) { return a + b; }\n",
        ),
        (
            "src/app.js",
            "import { addNumbers } from './math.js';\nexport function computeTotal(x) { return addNumbers(x, 1); }\n",
        ),
        (
            "src/view.js",
            "import { computeTotal } from './app.js';\nexport function renderTotal() { return computeTotal(2); }\n",
        ),
        ("src/unrelated.js", "export function standalone() {}\n"),
    ];
    let mut facts = BTreeMap::new();
    for (rel, text) in sources {
        std::fs::write(root.join(rel), text).unwrap();
        facts.insert(rel.to_string(), extract("js", text));
    }
    let graph = build(&root, &facts, 0);
    let _ = std::fs::remove_dir_all(&root);
    let _ = Path::new("");
    graph
}

#[test]
fn words_are_taken_apart_by_case_and_separators() {
    assert_eq!(
        words("computeTotal_value-2"),
        ["compute", "total", "value", "2"]
    );
}

#[test]
fn a_question_finds_the_matching_elements_with_their_relations() {
    let graph = graph();
    let answer = query(&graph, "where is the total computed?", 4000);
    assert!(
        answer.contains("computeTotal") && answer.contains("renderTotal"),
        "{answer}"
    );
    assert!(
        answer.contains("calls: addNumbers") || answer.contains("called by: renderTotal"),
        "{answer}"
    );
    assert!(!answer.contains("standalone"), "{answer}");
    assert!(query(&graph, "blablabla", 4000).starts_with("Nothing in the graph"));
    assert!(
        query(&graph, "the and for", 4000).contains("at least one word"),
        "only stop words"
    );
}

#[test]
fn the_budget_cuts_the_answer_and_says_what_is_left() {
    let graph = graph();
    let answer = query(&graph, "total add compute render", 200);
    assert!(answer.contains("more results"), "{answer}");
    assert!(answer.len() < 700, "{} chars", answer.len());
}

#[test]
fn a_node_lists_what_it_is_connected_to_and_a_path_links_two_names() {
    let graph = graph();
    let info = node(&graph, "computeTotal", 4000);
    assert!(
        info.contains("calls: addNumbers") && info.contains("called by: renderTotal"),
        "{info}"
    );
    assert!(node(&graph, "n'existe pas", 4000).starts_with("No element named"));
    let route = path(&graph, "renderTotal", "addNumbers");
    assert!(
        route.contains("renderTotal")
            && route.contains("computeTotal")
            && route.contains("addNumbers"),
        "{route}"
    );
    assert!(path(&graph, "standalone", "addNumbers").starts_with("No path between"));
}
