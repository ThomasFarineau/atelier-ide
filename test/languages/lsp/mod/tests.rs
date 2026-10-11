use super::*;

#[test]
fn positions_count_utf16_units() {
    let text = "a😀b\nsecond line\n";
    assert_eq!(to_position(text, 0), (0, 0));
    assert_eq!(to_position(text, 1), (0, 1));
    assert_eq!(to_position(text, 5), (0, 3), "an emoji is two units");
    assert_eq!(to_position(text, text.find("line").unwrap()), (1, 7));
    assert_eq!(from_position(text, 0, 3), 5);
    assert_eq!(from_position(text, 1, 7), text.find("line").unwrap());
    assert_eq!(from_position(text, 9, 0), text.len(), "past the end");
    assert_eq!(
        from_position(text, 0, 99),
        text.find('\n').unwrap(),
        "past the line"
    );
}

#[test]
fn addresses_are_encoded_and_compared_in_one_form() {
    let uri = path_to_uri(Path::new(r"C:\Users\Thomas\my project\a#b.js"));
    assert_eq!(uri, "file:///C:/Users/Thomas/my%20project/a%23b.js");
    assert_eq!(
        normalize_uri(&uri),
        normalize_uri("file:///c%3A/Users/Thomas/my%20project/a%23b.js")
    );
    assert_eq!(
        path_to_uri(Path::new("/home/t/x.js")),
        "file:///home/t/x.js"
    );
}

#[test]
fn addresses_come_back_to_paths() {
    assert_eq!(
        uri_to_path("file:///C:/Users/Thomas/my%20project/a.js"),
        Some(PathBuf::from("C:/Users/Thomas/my project/a.js"))
    );
    assert_eq!(
        uri_to_path("file:///home/t/x.js"),
        Some(PathBuf::from("/home/t/x.js"))
    );
    assert_eq!(uri_to_path("https://x"), None);
}

#[test]
fn messages_are_read_with_their_length() {
    let body = br#"{"jsonrpc":"2.0","id":1,"result":null}"#;
    let framed = [
        format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes(),
        body.to_vec(),
    ]
    .concat();
    let mut reader = std::io::Cursor::new(framed);
    let message = read_message(&mut reader).unwrap().unwrap();
    assert_eq!(message["id"], 1);
    assert!(
        read_message(&mut reader).unwrap().is_none(),
        "end of stream"
    );
}
