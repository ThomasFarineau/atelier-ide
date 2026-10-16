use super::*;

#[test]
fn toggling_sets_and_removes() {
    let mut list = Vec::new();
    let path = Path::new("/p/a.rs");
    assert!(toggle(&mut list, path, 3, "  let x = 1;  "));
    assert_eq!(list[0].text, "let x = 1;");
    assert!(!toggle(&mut list, path, 3, "let x = 1;"));
    assert!(list.is_empty());
}

#[test]
fn a_bookmark_follows_its_line() {
    let path = Path::new("/p/a.rs");
    let mut list = vec![Bookmark {
        path: path.to_path_buf(),
        line: 1,
        text: "fn b() {}".into(),
        number: None,
    }];
    // Two lines were added above it.
    let lines = ["// new", "// new", "fn a() {}", "fn b() {}"];
    assert!(follow(&mut list, path, &lines));
    assert_eq!(list[0].line, 3);
    // Nothing changes when the line is still there.
    assert!(!follow(&mut list, path, &lines));
    // Another file's bookmarks are left alone.
    assert!(!follow(&mut list, Path::new("/p/b.rs"), &["x"]));
}

#[test]
fn a_number_belongs_to_one_bookmark() {
    let mut list = Vec::new();
    let (a, b) = (Path::new("/p/a.rs"), Path::new("/p/b.rs"));
    assert!(number(&mut list, a, 1, "one", 3));
    // The same number on another line moves it there.
    assert!(number(&mut list, b, 5, "five", 3));
    assert_eq!(list.iter().filter(|x| x.number == Some(3)).count(), 1);
    assert_eq!(list.iter().find(|x| x.number == Some(3)).unwrap().path, b);
    // A plain bookmark takes the number.
    assert!(toggle(&mut list, a, 9, "nine"));
    assert!(number(&mut list, a, 9, "nine", 1));
    assert_eq!(list.len(), 3);
    // Again on the same line takes it off, and the bookmark stays.
    assert!(!number(&mut list, a, 9, "nine", 1));
    assert_eq!(list.len(), 3);
}
