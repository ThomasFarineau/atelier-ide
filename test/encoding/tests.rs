use super::*;

#[test]
fn files_are_read_by_their_mark_else_utf8_else_windows_1252() {
    assert_eq!(detect("héllo".as_bytes()), ("héllo".into(), UTF8));
    let (text, encoding) = detect(b"\xEF\xBB\xBFhi");
    assert_eq!(
        (text.as_str(), encoding.label().as_str()),
        ("hi", "UTF-8 BOM")
    );
    let (text, encoding) = detect(b"\xFF\xFEh\0i\0");
    assert_eq!(
        (text.as_str(), encoding.label().as_str()),
        ("hi", "UTF-16 LE")
    );
    let (text, encoding) = detect(b"caf\xE9");
    assert_eq!(
        (text.as_str(), encoding.label().as_str()),
        ("café", "windows-1252")
    );
}

#[test]
fn every_encoding_writes_back_what_it_read() {
    for encoding in ALL {
        let text = "abc\n";
        let bytes = encoding.encode(text).unwrap();
        assert_eq!(encoding.decode(&bytes), text, "{}", encoding.label());
    }
    let latin = ALL.iter().find(|e| e.label() == "windows-1252").unwrap();
    assert_eq!(latin.encode("€ 日").unwrap_err(), '日');
    assert_eq!(latin.encode("café").unwrap(), b"caf\xE9");
}

#[test]
fn line_endings_are_read_and_changed() {
    assert_eq!(LineEnding::of("a\r\nb"), LineEnding::CrLf);
    assert_eq!(LineEnding::of("a\nb\r\n"), LineEnding::Lf);
    assert_eq!(LineEnding::of("a\rb"), LineEnding::Cr);
    assert_eq!(LineEnding::of("a"), LineEnding::Lf);
    assert_eq!(LineEnding::CrLf.apply("a\nb\r\nc\rd"), "a\r\nb\r\nc\r\nd");
    assert_eq!(LineEnding::Lf.apply("a\r\nb"), "a\nb");
}
