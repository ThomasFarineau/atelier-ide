use super::*;
#[test]
fn arguments_are_quoted_as_literals() {
    let cmd = shell_command(
        "python",
        &["a b.py".into(), "$(echo injected)".into(), "it's.py".into()],
    );
    assert!(cmd.contains("'a b.py'"));
    assert!(cmd.contains("'$(echo injected)'"));
    if cfg!(windows) {
        assert!(cmd.contains("'it''s.py'"));
    } else {
        assert!(cmd.contains("'it'\\''s.py'"));
    }
}
