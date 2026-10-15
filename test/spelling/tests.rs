use super::*;

fn checker() -> Checker {
    Checker::new(
        &[
            (
                include_str!("../../assets/dictionaries/en_US.aff"),
                include_str!("../../assets/dictionaries/en_US.dic"),
            ),
            (
                include_str!("../../assets/dictionaries/fr_FR.aff"),
                include_str!("../../assets/dictionaries/fr_FR.dic"),
            ),
        ],
        None,
    )
}

#[test]
fn both_languages_are_known() {
    let c = checker();
    assert!(c.is_correct("function"));
    assert!(c.is_correct("bonjour"));
    assert!(c.is_correct("éditeur"));
    assert!(!c.is_correct("fonctoin"));
    assert!(c.suggestions("fonctoin").iter().any(|s| s == "fonction"));
}

#[test]
fn comments_and_strings_are_prose_the_code_is_not() {
    let text = "fn main() {\n    // a commnet here\n    let s = \"a strng\"; // other\n}\n";
    let ranges = prose(text, "rs");
    let pieces: Vec<&str> = ranges.iter().map(|r| &text[r.clone()]).collect();
    assert_eq!(pieces, [" a commnet here", "a strng", " other"]);
}

#[test]
fn only_real_typos_are_reported() {
    let c = checker();
    let text = "// The quikc fox, camelCase, snake_case, UPPER, path/to/file.rs, 3rd, i18n and don't\nlet x = 1;\n";
    let found: Vec<&str> = typos(&c, text, "rs")
        .iter()
        .map(|r| &text[r.clone()])
        .collect();
    assert_eq!(found, ["quikc"]);
}

#[test]
fn markdown_skips_fenced_code() {
    let c = checker();
    let text = "Un textee ici.\n\n```rust\nlet fooo = 1;\n```\n\nFin du paragrahpe.\n";
    let found: Vec<&str> = typos(&c, text, "md")
        .iter()
        .map(|r| &text[r.clone()])
        .collect();
    assert_eq!(found, ["textee", "paragrahpe"]);
}

#[test]
fn added_words_are_right() {
    let dir = std::env::temp_dir().join(format!("atelier-spell-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let file = dir.join("dictionary.txt");
    let c = Checker::new(&[], Some(file.clone()));
    assert!(!c.is_correct("Atelierz"));
    c.add("Atelierz");
    assert!(c.is_correct("atelierz"));
    // Read again by another checker.
    assert!(Checker::new(&[], Some(file)).is_correct("ATELIERZ"));
    let _ = fs::remove_dir_all(dir);
}
