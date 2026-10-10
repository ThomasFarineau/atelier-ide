use super::*;

#[test]
fn the_edition_comes_from_the_manifest() {
    let dir = std::env::temp_dir().join(format!("atelier-rustfmt-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"x\"\nedition = \"2024\"\n",
    )
    .unwrap();
    assert_eq!(edition(Some(&dir.join("src/main.rs"))), "2024");
    std::fs::write(dir.join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
    assert_eq!(edition(Some(&dir.join("src/main.rs"))), "2015");
    assert_eq!(edition(None), "2021");
    let _ = std::fs::remove_dir_all(&dir);
}

/// With the real rustfmt, which the toolchain installs.
#[test]
fn rustfmt_formats_and_reports_what_does_not_parse() {
    let Ok(formatted) = rustfmt("fn main(){let x=1;}", None) else {
        return; // No rustfmt here.
    };
    assert_eq!(formatted, "fn main() {\n    let x = 1;\n}\n");
    let windows = rustfmt("fn main(){\r\nlet x=1;}\r\n", None).unwrap();
    assert_eq!(windows, "fn main() {\r\n    let x = 1;\r\n}\r\n");
    assert!(rustfmt("fn main( {", None).is_err());
}
