use super::*;

#[test]
fn a_pinned_version_is_missing_until_one_satisfies_it() {
    let mut state = State {
        installed: vec!["3.11.9".into()],
        active: None,
        pinned: Some((".python-version", "3.12".into())),
    };
    assert!(state.missing().is_some());
    state.installed.insert(0, "3.12.15".into());
    assert!(state.missing().is_none());
}
