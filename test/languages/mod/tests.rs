use super::*;

#[test]
fn fuzzy_matching_prefers_prefixes_and_word_starts() {
    let score = |q, l| fuzzy_score(q, l);
    assert!(score("con", "console").unwrap() > score("con", "decon").unwrap_or(i32::MIN));
    assert!(score("gEBI", "getElementById").is_some());
    assert!(score("gebi", "getElementById").is_some());
    assert!(score("xyz", "console").is_none());
    assert!(score("log", "console.log").is_some());
    assert_eq!(score("", "anything"), Some(0));
    assert!(score("fe", "forEach").unwrap() > score("fe", "buffer").unwrap());
}

#[test]
fn oneshot_is_read_once() {
    let slot = Oneshot::pending();
    assert!(slot.take().is_none());
    slot.clone().fulfill(3);
    assert_eq!(slot.take(), Some(3));
    assert_eq!(slot.take(), None);
    assert_eq!(Oneshot::ready("x").take(), Some("x"));
}
