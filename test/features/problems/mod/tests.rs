use super::*;

fn problem(path: &str, line: usize, severity: u8) -> Problem {
    Problem {
        path: PathBuf::from(path),
        line,
        column: 0,
        length: 1,
        severity,
        message: format!("problème {line}"),
        source: String::new(),
    }
}

#[test]
fn problems_are_filtered_and_ordered_by_file_severity_and_line() {
    let all = vec![
        problem("b.js", 3, 2),
        problem("a.js", 9, 2),
        problem("a.js", 5, 1),
        problem("a.js", 1, 2),
        problem("c.js", 0, 4),
    ];
    let shown = visible(&all, true, true);
    let order: Vec<(String, usize)> = shown
        .iter()
        .map(|p| (p.path.display().to_string(), p.line))
        .collect();
    assert_eq!(
        order,
        [
            ("a.js".to_string(), 5),
            ("a.js".to_string(), 1),
            ("a.js".to_string(), 9),
            ("b.js".to_string(), 3)
        ],
        "errors first, hints left out"
    );
    assert_eq!(visible(&all, true, false).len(), 1);
    assert_eq!(visible(&all, false, true).len(), 3);
}
