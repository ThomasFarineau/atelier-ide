use super::*;
use crate::plugins::builtins::mcp::quality::scratch;

#[test]
fn writes_into_nested_metrics_paths() {
    let dir = scratch();
    let out = Out::default();
    write(dir.path(), "mutate/a.edn", "{}", &out);
    assert_eq!(
        std::fs::read_to_string(dir.path().join(".metrics/mutate/a.edn")).unwrap(),
        "{}"
    );
    assert_eq!(out.take(), "Wrote .metrics/mutate/a.edn\n");
}
