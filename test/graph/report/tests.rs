use super::*;
use crate::graph::build::{Link, Node};

fn node(id: &str, kind: &str, file: &str, size: u64) -> Node {
    Node {
        id: id.into(),
        label: id.trim_start_matches("s:").trim_start_matches("f:").into(),
        kind: kind.into(),
        file: file.into(),
        line: 1,
        community: 0,
        degree: 0,
        size,
    }
}

/// Two big files, a function in each that calls the other.
fn project(file_size: u64) -> Graph {
    let mut graph = Graph {
        nodes: vec![
            node("f:a.js", "file", "a.js", file_size),
            node("f:b.js", "file", "b.js", file_size),
            node("s:alpha", "function", "a.js", 0),
            node("s:beta", "function", "b.js", 0),
        ],
        links: vec![
            Link {
                source: 0,
                target: 2,
                kind: "contains".into(),
                confidence: "EXTRACTED".into(),
            },
            Link {
                source: 1,
                target: 3,
                kind: "contains".into(),
                confidence: "EXTRACTED".into(),
            },
            Link {
                source: 2,
                target: 3,
                kind: "calls".into(),
                confidence: "EXTRACTED".into(),
            },
        ],
        ..Graph::default()
    };
    for link in graph.links.clone().iter().filter(|l| l.kind != "contains") {
        graph.nodes[link.source].degree += 1;
        graph.nodes[link.target].degree += 1;
    }
    graph
}

#[test]
fn the_saving_is_measured_on_questions_not_on_the_whole_project() {
    let b = benchmark(&project(40_000)).expect("big files: the graph is cheaper");
    assert_eq!(b.questions, 2);
    assert_eq!(
        b.files, 20_000,
        "the defining file and the one it is linked to, ~10k tokens each"
    );
    assert!(b.graph < 100, "a short answer: {}", b.graph);
    assert_eq!(b.percent(), 99);
    assert_eq!(
        Benchmark {
            questions: 1,
            graph: 300,
            files: 1_000
        }
        .percent(),
        70
    );
    assert_eq!(benchmark(&project(8)), None, "tiny files: nothing to save");
    assert_eq!(benchmark(&Graph::default()), None);
    assert_eq!(
        (tokens(950), tokens(1_234), tokens(2_500_000)),
        ("950".into(), "1.2k".into(), "2.5M".into())
    );
}

#[test]
fn the_report_is_in_english_and_says_what_the_saving_was_measured_on() {
    let report = markdown(&project(40_000));
    assert!(report.starts_with("# Context graph"), "{report}");
    assert!(
        report.contains("Estimated token saving: up to ~99% per question")
            && report.contains("Measured on 2 symbols"),
        "{report}"
    );
    assert!(
        report.contains("upper bound"),
        "no inflated claim: {report}"
    );
    assert!(
        report.contains("## Central elements")
            && report.contains("## File groups")
            && report.contains("generated"),
        "{report}"
    );
    assert!(
        !report.contains("fichiers") && !report.contains("Éléments"),
        "no French left"
    );
    assert!(markdown(&project(8)).contains("The project is small"));
}
