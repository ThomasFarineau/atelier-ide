use super::*;

#[test]
fn options_decide_what_matches() {
    let text = "Cat cat concat CAT";
    let count = |query: &str, o: Options| find_all(text, &build(query, o).unwrap(), 100).len();
    assert_eq!(
        count("cat", Options::default()),
        4,
        "case does not matter by default"
    );
    assert_eq!(
        count(
            "cat",
            Options {
                case: true,
                ..Default::default()
            }
        ),
        2
    );
    assert_eq!(
        count(
            "cat",
            Options {
                word: true,
                ..Default::default()
            }
        ),
        3
    );
    assert_eq!(
        count("c.t", Options::default()),
        0,
        "plain text: the dot is a dot"
    );
    assert_eq!(
        count(
            "c.t",
            Options {
                regex: true,
                ..Default::default()
            }
        ),
        4
    );
    assert!(
        build(
            "(",
            Options {
                regex: true,
                ..Default::default()
            }
        )
        .is_err()
    );
    assert_eq!(
        find_all(
            "aaa",
            &build(
                "a*",
                Options {
                    regex: true,
                    ..Default::default()
                }
            )
            .unwrap(),
            10
        )
        .len(),
        1,
        "empty matches are skipped"
    );
}

#[test]
fn replacement_is_literal_unless_a_regex() {
    let plain = Options::default();
    let re = build("a.b", plain).unwrap();
    assert_eq!(replace_all("a.b axb", &re, "$1", plain), "$1 axb");
    let rx = Options {
        regex: true,
        ..Default::default()
    };
    let re = build(r"(\w+)@(\w+)", rx).unwrap();
    assert_eq!(replace_all("me@host", &re, "$2:$1", rx), "host:me");
}

#[test]
fn masks_match_names_and_paths() {
    assert!(glob_match("*.rs", "Main.RS"));
    assert!(glob_match("a?c*", "abcdef"));
    assert!(!glob_match("*.rs", "main.js"));
    assert!(mask_matches("", Path::new("src/a.txt")));
    assert!(mask_matches("*.js, *.ts", Path::new("src/deep/a.ts")));
    assert!(!mask_matches("*.js", Path::new("src/a.ts")));
    assert!(mask_matches("src/*", Path::new("src/deep/a.ts")));
    assert!(!mask_matches("lib/*", Path::new("src/a.ts")));
}

#[test]
fn hits_carry_their_line_and_column() {
    let text = "fn one() {}\n    let total = 1;\r\nlet total2 = total;\n";
    let re = build(
        "total",
        Options {
            word: true,
            ..Default::default()
        },
    )
    .unwrap();
    let hits = hits_in(text, &re, 10);
    assert_eq!(hits.len(), 2);
    assert_eq!((hits[0].line, hits[0].column, hits[0].length), (1, 8, 5));
    assert_eq!(hits[0].text, "let total = 1;");
    assert_eq!(hits[0].shown_column, 4);
    assert_eq!((hits[1].line, hits[1].column), (2, 13));
}

#[test]
fn the_project_is_searched_without_its_libraries() {
    let root = std::env::temp_dir().join(format!("atelier-search-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::create_dir_all(root.join("node_modules").join("dep")).unwrap();
    std::fs::write(
        root.join("src").join("a.js"),
        "const needle = 1;\nneedle();\n",
    )
    .unwrap();
    std::fs::write(root.join("src").join("b.txt"), "a needle here\n").unwrap();
    std::fs::write(root.join("src").join("c.bin"), b"needle\0binary").unwrap();
    std::fs::write(
        root.join("node_modules").join("dep").join("x.js"),
        "needle\n",
    )
    .unwrap();
    let re = build("needle", Options::default()).unwrap();
    let cancel = AtomicBool::new(false);
    let mut found: Vec<(String, usize)> = Vec::new();
    let looked = search_project(&root, &re, "", &cancel, 100, |h| {
        found.push((
            h.path.file_name().unwrap().to_string_lossy().into_owned(),
            h.hits.len(),
        ));
    });
    found.sort();
    assert_eq!(found, [("a.js".to_string(), 2), ("b.txt".to_string(), 1)]);
    assert_eq!(looked, 2);
    let mut only_js = Vec::new();
    search_project(&root, &re, "*.js", &cancel, 100, |h| only_js.push(h.path));
    assert_eq!(only_js.len(), 1);
    // Replacing touches the same files.
    let (files, total) = replace_in_project(&root, &re, "pin", Options::default(), "*.js");
    assert_eq!((files, total), (1, 2));
    assert_eq!(
        std::fs::read_to_string(root.join("src").join("a.js")).unwrap(),
        "const pin = 1;\npin();\n"
    );
    assert!(
        std::fs::read_to_string(root.join("src").join("b.txt"))
            .unwrap()
            .contains("needle")
    );
    let _ = std::fs::remove_dir_all(&root);
}
