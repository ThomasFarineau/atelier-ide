use super::*;

#[test]
fn the_lines_of_the_bridge_are_events() {
    assert_eq!(
        parse_event(r#"{"type": "ready", "version": "3.12.7"}"#),
        Some(Event::Ready("3.12.7".into()))
    );
    assert_eq!(
        parse_event(r#"{"type": "missing", "error": "x"}"#),
        Some(Event::Missing)
    );
    assert_eq!(
        parse_event(r#"{"type": "done", "cell": 4, "count": 7}"#),
        Some(Event::Done {
            cell: 4,
            count: Some(7)
        })
    );
    assert_eq!(
        parse_event(r#"{"type": "output", "cell": 1, "kind": "stream", "content": {"text": "a"}}"#),
        Some(Event::Output {
            cell: 1,
            kind: "stream".into(),
            content: json!({"text": "a"})
        })
    );
    assert_eq!(parse_event("Traceback (most recent call last):"), None);
}

/// With a Python that has Jupyter (`ATELIER_TEST_JUPYTER_PYTHON`), a cell runs for real.
#[test]
fn a_cell_runs_in_a_kernel() {
    let Some(python) = std::env::var_os("ATELIER_TEST_JUPYTER_PYTHON") else {
        return;
    };
    let cwd = std::env::temp_dir();
    let mut kernel = Kernel::start(Path::new(&python), &cwd, || {}).unwrap();
    kernel.execute(1, "print(6 * 7)\n6 * 7");
    let mut events = Vec::new();
    let start = std::time::Instant::now();
    while !events.iter().any(|e| matches!(e, Event::Done { .. })) {
        assert!(start.elapsed() < Duration::from_secs(60), "{events:?}");
        thread::sleep(Duration::from_millis(100));
        events.extend(kernel.events());
    }
    assert!(events.iter().any(|e| matches!(e, Event::Ready(_))));
    assert!(events.iter().any(|e| matches!(
        e,
        Event::Output { kind, content, .. } if kind == "stream" && content["text"] == "42\n"
    )));
}
