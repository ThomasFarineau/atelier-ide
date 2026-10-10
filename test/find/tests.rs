use super::*;

#[test]
#[allow(clippy::field_reassign_with_default)]
fn matches_follow_the_query_the_options_and_the_text() {
    let mut find = Find::default();
    find.query = "cat".into();
    find.refresh(0, 1, "Cat cat concat");
    assert_eq!(find.matches().len(), 3);
    find.options.word = true;
    find.refresh(0, 1, "Cat cat concat");
    assert_eq!(find.matches().len(), 2);
    // The same key is not computed again; a new revision is.
    find.refresh(0, 2, "cat");
    assert_eq!(find.matches().len(), 1);
    find.query = "(".into();
    find.options.regex = true;
    find.refresh(0, 2, "cat");
    assert!(find.matches().is_empty() && find.error.is_some());
}

#[test]
#[allow(clippy::field_reassign_with_default)]
fn next_and_previous_wrap_around_the_selection() {
    let mut find = Find::default();
    find.query = "a".into();
    find.refresh(0, 1, "a_a_a");
    assert_eq!(find.matches(), [0..1, 2..3, 4..5]);
    assert_eq!(find.neighbor(&(0..1), true), Some(2..3));
    assert_eq!(find.neighbor(&(4..5), true), Some(0..1), "wraps");
    assert_eq!(find.neighbor(&(1..1), true), Some(2..3), "from a caret");
    assert_eq!(find.neighbor(&(2..3), false), Some(0..1));
    assert_eq!(find.neighbor(&(0..1), false), Some(4..5), "wraps backwards");
    assert_eq!(find.position(&(2..3)), Some(1));
}
