use super::*;
use crate::graph::extract::extract;

fn project(files: &[(&str, &str)]) -> (PathBuf, BTreeMap<String, Facts>) {
    // One folder per call: the tests run side by side and each removes its own.
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("atelier-graph-build-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let mut facts = BTreeMap::new();
    for (rel, text) in files {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, text).unwrap();
        let language = crate::graph::extract::language_of(
            Path::new(rel).extension().and_then(|e| e.to_str()).unwrap(),
        )
        .unwrap();
        facts.insert(rel.to_string(), extract(language, text));
    }
    (root, facts)
}

fn has(graph: &Graph, from: &str, kind: &str, to: &str) -> bool {
    graph.links.iter().any(|l| {
        l.kind == kind && graph.nodes[l.source].id == from && graph.nodes[l.target].id == to
    })
}

#[test]
fn javascript_files_are_linked_by_imports_and_by_the_calls_they_make() {
    let (root, facts) = project(&[
        (
            "src/math.js",
            "export function add(a, b) { return a + b; }\nexport function unrelated() {}\n",
        ),
        (
            "src/app.js",
            "import { add } from './math.js';\nexport function total(x) { return add(x, 1); }\n",
        ),
        (
            "src/other.js",
            "function lonely() { return computeThing(); }\n",
        ),
        (
            "src/deep.js",
            "export function computeThing() { return 1; }\n",
        ),
    ]);
    let graph = build(&root, &facts, 0);
    assert!(has(&graph, "f:src/app.js", "imports", "f:src/math.js"));
    assert!(has(
        &graph,
        "f:src/math.js",
        "contains",
        "s:src/math.js#add"
    ));
    let call = graph
        .links
        .iter()
        .find(|l| l.kind == "calls" && graph.nodes[l.source].label == "total")
        .expect("a call");
    assert_eq!(
        (
            graph.nodes[call.target].id.as_str(),
            call.confidence.as_str()
        ),
        ("s:src/math.js#add", "EXTRACTED"),
        "add is imported by the caller"
    );
    let guessed = graph
        .links
        .iter()
        .find(|l| l.kind == "calls" && graph.nodes[l.source].label == "lonely")
        .expect("a guess");
    assert_eq!(
        (
            graph.nodes[guessed.target].label.as_str(),
            guessed.confidence.as_str()
        ),
        ("computeThing", "INFERRED"),
        "no import, but only one match in the project"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn rust_modules_and_uses_lead_to_the_files_they_name() {
    let (root, facts) = project(
        &[
            ("Cargo.toml", ""),
            (
                "src/main.rs",
                "mod graph;\nuse crate::graph::build::make;\nfn main() { make(); }\n",
            ),
            ("src/graph/mod.rs", "pub mod build;\n"),
            (
                "src/graph/build.rs",
                "use super::helper::assist;\npub fn make() { assist(); }\n",
            ),
            ("src/graph/helper.rs", "pub fn assist() {}\n"),
        ][1..],
    );
    let graph = build(&root, &facts, 0);
    assert!(
        has(&graph, "f:src/main.rs", "declares", "f:src/graph/mod.rs"),
        "{:?}",
        graph.links
    );
    assert!(has(
        &graph,
        "f:src/main.rs",
        "imports",
        "f:src/graph/build.rs"
    ));
    assert!(has(
        &graph,
        "f:src/graph/mod.rs",
        "declares",
        "f:src/graph/build.rs"
    ));
    assert!(has(
        &graph,
        "f:src/graph/build.rs",
        "imports",
        "f:src/graph/helper.rs"
    ));
    assert!(has(
        &graph,
        "s:src/main.rs#main",
        "calls",
        "s:src/graph/build.rs#make"
    ));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn files_that_lean_on_each_other_form_a_group_and_the_rest_group_by_folder() {
    let (root, facts) = project(&[
        (
            "a/one.js",
            "import './two.js';\nexport function one() { two(); }\n",
        ),
        (
            "a/two.js",
            "import './one.js';\nexport function two() { one(); }\n",
        ),
        ("b/x.js", "export const x = 1;\n"),
        ("b/y.js", "export const y = 2;\n"),
        ("README.md", "# Projet\n\nVoir [a](a/one.js).\n"),
    ]);
    let graph = build(&root, &facts, 0);
    let community = |file: &str| {
        graph
            .nodes
            .iter()
            .find(|n| n.id == format!("f:{file}"))
            .unwrap()
            .community
    };
    assert_eq!(community("a/one.js"), community("a/two.js"));
    assert_eq!(
        community("b/x.js"),
        community("b/y.js"),
        "unlinked files of a folder go together"
    );
    assert_ne!(community("a/one.js"), community("b/x.js"));
    // The same facts always give the same graph.
    assert_eq!(build(&root, &facts, 0), graph);
    let _ = std::fs::remove_dir_all(&root);
}
