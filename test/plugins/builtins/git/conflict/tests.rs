use super::*;

const FILE: &str = "start\n<<<<<<< HEAD\nmine\n||||||| base\nold\n=======\ntheirs\n>>>>>>> feature\nmiddle\n<<<<<<< HEAD\na\n=======\nb\n>>>>>>> feature\nend\n";

#[test]
fn parses_and_resolves_blocks() {
    let mut segments = parse(FILE);
    assert_eq!(conflicts(&segments), 2);
    assert_eq!(
        render(&segments),
        FILE,
        "unresolved blocks are kept verbatim"
    );
    let Segment::Conflict(first) = &segments[1] else {
        panic!()
    };
    assert_eq!(first.ours, "mine\n");
    assert_eq!(first.base.as_deref(), Some("old\n"));
    assert_eq!(first.theirs, "theirs\n");
    assert_eq!(first.theirs_label, "feature");
    for (segment, choice) in segments
        .iter_mut()
        .filter_map(|s| match s {
            Segment::Conflict(c) => Some(c),
            Segment::Text(_) => None,
        })
        .zip([Choice::Theirs, Choice::OursThenTheirs])
    {
        segment.choice = Some(choice);
    }
    assert!(resolved(&segments));
    assert_eq!(render(&segments), "start\ntheirs\nmiddle\na\nb\nend\n");
}

#[test]
fn unterminated_block_is_plain_text() {
    let text = "a\n<<<<<<< HEAD\nb\n";
    let segments = parse(text);
    assert_eq!(conflicts(&segments), 0);
    assert_eq!(render(&segments), text);
}
