use super::*;

const CSS: &str = "
/* latin */
@font-face {
  font-family: 'Fira Code';
  font-style: normal;
  font-weight: 400;
  src: url(https://fonts.gstatic.com/s/firacode/regular.ttf) format('truetype');
}
@font-face {
  font-family: 'Fira Code';
  font-style: italic;
  font-weight: 400;
  src: url(italic.ttf) format('truetype');
}
@font-face {
  font-family: \"Fira Code\";
  font-weight: 700;
  src: url('/fonts/bold.woff2') format('woff2'), url(\"bold.otf\");
}
@font-face {
  font-family: 'Only Woff';
  src: url(only.woff2) format('woff2');
}
";

#[test]
fn css_rules_give_their_truetype_and_opentype_files() {
    let faces = font_faces(CSS, "https://cdn.example.com/css/fonts.css");
    assert_eq!(faces.len(), 4);
    assert_eq!(
        faces[0],
        FontFace {
            family: "Fira Code".into(),
            weight: 400,
            italic: false,
            url: Some("https://fonts.gstatic.com/s/firacode/regular.ttf".into()),
        }
    );
    assert!(faces[1].italic);
    assert_eq!(
        faces[1].url.as_deref(),
        Some("https://cdn.example.com/css/italic.ttf")
    );
    // The WOFF2 file is skipped for the OpenType one.
    assert_eq!(
        faces[2].url.as_deref(),
        Some("https://cdn.example.com/css/bold.otf")
    );
    assert_eq!(faces[3].url, None);
}

#[test]
fn addresses_resolve_from_the_css() {
    let base = "https://cdn.example.com/css/fonts.css";
    assert_eq!(
        absolute("/f/a.ttf", base),
        "https://cdn.example.com/f/a.ttf"
    );
    assert_eq!(
        absolute("//other.org/a.ttf", base),
        "https://other.org/a.ttf"
    );
    assert_eq!(absolute("a.ttf", base), "https://cdn.example.com/css/a.ttf");
}

#[test]
fn regular_and_semibold_weights_are_picked() {
    let face = |weight, url: &str| FontFace {
        family: String::new(),
        weight,
        italic: false,
        url: Some(url.into()),
    };
    let faces = [
        face(300, "light"),
        face(400, "regular-latin"),
        face(400, "regular-greek"),
        face(700, "bold"),
    ];
    assert_eq!(
        pick_weights(&faces),
        (
            vec!["regular-latin".to_string(), "regular-greek".to_string()],
            vec!["bold".to_string()]
        )
    );
    // A single weight: no semibold.
    assert_eq!(
        pick_weights(&[face(500, "medium")]),
        (vec!["medium".into()], vec![])
    );
}

#[test]
fn catalog_categories_are_read_from_both_spellings() {
    assert_eq!(Category::parse("Sans Serif"), Category::Sans);
    assert_eq!(Category::parse("sans-serif"), Category::Sans);
    assert_eq!(Category::parse("monospace"), Category::Monospace);
    assert_eq!(Category::parse("Handwriting"), Category::Handwriting);
}

#[test]
fn only_truetype_and_opentype_files_are_accepted() {
    assert!(
        check_font(
            include_bytes!("../../assets/fonts/JetBrainsMono-Regular.ttf"),
            "a"
        )
        .is_ok()
    );
    assert!(
        check_font(b"wOF2....", "a")
            .unwrap_err()
            .to_string()
            .contains("WOFF")
    );
    assert!(check_font(b"<html>", "a").is_err());
    assert_eq!(file_stem("https://x.org/f/Cool-Font.ttf?v=2"), "Cool-Font");
}

#[test]
fn a_font_file_is_installed_listed_and_removed() {
    let root = std::env::temp_dir().join(format!("atelier-fonts-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let file = root.join("download").join("mono.ttf");
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(
        &file,
        include_bytes!("../../assets/fonts/JetBrainsMono-Regular.ttf"),
    )
    .unwrap();
    let store = root.join("fonts");
    let font = install_in(&store, &Source::File(file)).unwrap();
    // The name and the kind come from the file itself.
    assert_eq!(font.family, "JetBrains Mono");
    assert_eq!(font.category, Category::Monospace);
    assert_eq!(installed_in(&store), vec![font]);
    fs::remove_dir_all(family_dir(&store, "JetBrains Mono")).unwrap();
    assert!(installed_in(&store).is_empty());
    let _ = fs::remove_dir_all(&root);
}

/// Asks the real services: `cargo test fonts -- --ignored`.
#[test]
#[ignore]
fn fonts_install_from_the_services() {
    let root = std::env::temp_dir().join(format!("atelier-fonts-net-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    for provider in Provider::ALL {
        let entries = catalog(provider).unwrap();
        let entry = entries.iter().find(|e| e.family == "Fira Code").unwrap();
        assert_eq!(entry.category, Category::Monospace);
        let store = root.join(provider.name());
        let font = install_in(&store, &Source::Catalog(provider, entry.clone())).unwrap();
        assert!(
            !font.regular.is_empty() && !font.semibold.is_empty(),
            "{font:?}"
        );
    }
    let css = "https://fonts.googleapis.com/css2?family=Lora:wght@400;600";
    let font = install_in(&root.join("css"), &Source::Address(css.into())).unwrap();
    assert_eq!(font.family, "Lora");
    let _ = fs::remove_dir_all(&root);
}
