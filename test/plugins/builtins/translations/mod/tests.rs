use super::*;
use catalog::{Format, Layout, discover, is_locale, placeholders};

fn project(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("atelier-i18n-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (path, text) in files {
        let path = dir.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    dir
}

const FR: &str = "{\n  \"menu\": {\n    \"open\": \"Ouvrir\",\n    \"save\": \"Enregistrer {name}\"\n  },\n  \"bye\": \"Au revoir et à bientôt\"\n}\n";

#[test]
fn locale_names_are_recognized() {
    for name in [
        "fr",
        "fr_FR",
        "en-US",
        "pt-BR",
        "zh-Hans",
        "zh_Hant_TW",
        "es-419",
    ] {
        assert!(is_locale(name), "{name}");
    }
    for name in ["common", "messages", "index", "FR"] {
        assert!(!is_locale(name), "{name}");
    }
    assert_eq!(
        placeholders("{{ count }} files of {name}, %s %1$d ${x}"),
        ["${x}", "%1$d", "%s", "{name}", "{{count}}"]
    );
}

#[test]
fn the_three_layouts_are_found_from_the_source() {
    let root = project(
        "layouts",
        &[
            ("locales/fr_FR.json", FR),
            ("locales/en_US.json", "{}"),
            ("locales/settings.json", "{}"),
            ("i18n/fr/common.json", "{}"),
            ("i18n/de/common.json", "{}"),
            ("i18n/de/other.json", "{}"),
            ("res/messages.properties", "a=b\n"),
            ("res/messages_it.properties", "a=c\n"),
        ],
    );
    let stem = discover(&root.join("locales/fr_FR.json")).unwrap();
    assert_eq!(stem.source.language, "fr_FR");
    assert_eq!(
        stem.others
            .iter()
            .map(|f| f.language.as_str())
            .collect::<Vec<_>>(),
        ["en_US"],
        "settings.json is no language"
    );
    assert!(matches!(stem.layout, Layout::Stem { .. }));
    assert_eq!(stem.path_for("es"), root.join("locales/es.json"));
    let folder = discover(&root.join("i18n/fr/common.json")).unwrap();
    assert_eq!(folder.others[0].path, root.join("i18n/de/common.json"));
    assert_eq!(folder.path_for("es"), root.join("i18n/es/common.json"));
    let bundle = discover(&root.join("res/messages.properties")).unwrap();
    assert_eq!(bundle.format, Format::Properties);
    assert_eq!(bundle.source.language, "default");
    assert_eq!(bundle.others[0].language, "it");
    assert_eq!(
        bundle.path_for("es"),
        root.join("res/messages_es.properties")
    );
    assert!(
        discover(&root.join("locales/settings.json")).is_err(),
        "no language in its path"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_check_finds_every_kind_of_problem() {
    let root = project(
        "check",
        &[
            ("locales/fr.json", FR),
            (
                "locales/en.json",
                "{\"menu\": {\"open\": \"\", \"save\": \"Save\"}, \"bye\": \"Au revoir et à bientôt\", \"old\": \"x\"}",
            ),
        ],
    );
    let catalog = discover(&root.join("locales/fr.json")).unwrap();
    let report = catalog::check(&catalog, "en").unwrap();
    assert_eq!(report.total, 3);
    assert_eq!(report.translated, 2);
    let problem = |key: &str| {
        report
            .problems
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, p)| p.clone())
    };
    assert_eq!(problem("menu.open"), Some(Problem::Empty));
    assert!(matches!(
        problem("menu.save"),
        Some(Problem::Placeholders { .. })
    ));
    assert_eq!(problem("bye"), Some(Problem::SameAsSource));
    assert_eq!(problem("old"), Some(Problem::Extra));
    assert_eq!(report.errors(), 3, "the same text is only a doubt");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn values_are_written_where_they_belong() {
    let root = project(
        "write",
        &[
            ("locales/fr.json", FR),
            (
                "res/m_fr.properties",
                "# titre\na=b\nlong=une \\\n  suite\n",
            ),
        ],
    );
    let json = root.join("locales/fr.json");
    catalog::set_value(&json, Format::Json, "menu.close", "Fermer").unwrap();
    catalog::set_value(&json, Format::Json, "bye", "Salut").unwrap();
    catalog::set_value(&json, Format::Json, "toolbar.run", "Lancer").unwrap();
    let text = std::fs::read_to_string(&json).unwrap();
    let value: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["menu"]["close"], "Fermer", "next to its siblings");
    assert_eq!(value["bye"], "Salut");
    assert_eq!(
        value["toolbar.run"], "Lancer",
        "no parent object: a flat key"
    );
    assert!(
        text.starts_with("{\n  \"menu\""),
        "the indentation of the file: {text}"
    );
    catalog::remove_key(&json, Format::Json, "menu.close").unwrap();
    catalog::remove_key(&json, Format::Json, "menu.open").unwrap();
    catalog::remove_key(&json, Format::Json, "menu.save").unwrap();
    let value: Value = serde_json::from_str(&std::fs::read_to_string(&json).unwrap()).unwrap();
    assert!(value.get("menu").is_none(), "an emptied object goes too");
    let properties = root.join("res/m_fr.properties");
    assert_eq!(
        catalog::read(&properties, Format::Properties).unwrap()[1],
        ("long".into(), "une suite".into())
    );
    catalog::set_value(&properties, Format::Properties, "a", "deux\nlignes").unwrap();
    catalog::set_value(&properties, Format::Properties, "c", "nouveau").unwrap();
    let text = std::fs::read_to_string(&properties).unwrap();
    assert!(text.starts_with("# titre\na=deux\\nlignes\n"), "{text}");
    assert!(text.ends_with("c=nouveau\n"));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_new_language_has_every_key_of_the_source() {
    let root = project("add", &[("locales/fr.json", FR)]);
    let catalog = discover(&root.join("locales/fr.json")).unwrap();
    let path = catalog::add_language(&catalog, "de", Fill::Empty).unwrap();
    let value: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(value["menu"]["save"], "", "same structure, empty texts");
    let catalog = discover(&root.join("locales/fr.json")).unwrap();
    assert!(
        catalog::add_language(&catalog, "de", Fill::Empty).is_err(),
        "already there"
    );
    assert!(
        catalog::add_language(&catalog, "../x", Fill::Empty).is_err(),
        "not a language"
    );
    let path = catalog::add_language(&catalog, "it", Fill::Source).unwrap();
    assert!(
        std::fs::read_to_string(path)
            .unwrap()
            .contains("Enregistrer {name}")
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_mcp_tool_sets_the_source_checks_and_adds_languages() {
    let root = project(
        "mcp",
        &[
            ("locales/fr.json", FR),
            ("locales/en.json", "{\"menu\": {\"open\": \"Open\"}}"),
        ],
    );
    let run = |args: Value| mcp_tool("i18n", &args, &root).unwrap();
    assert!(run(json!({"action": "check"})).is_err(), "no source yet");
    let languages: Value = serde_json::from_str(
        &run(json!({"action": "languages", "source": "locales/fr.json"})).unwrap(),
    )
    .unwrap();
    assert_eq!(languages["source"]["language"], "fr");
    assert_eq!(languages["languages"][0]["language"], "en");
    assert!(root.join(catalog::CONFIG).is_file(), "the choice is kept");
    let check: Value = serde_json::from_str(&run(json!({"action": "check"})).unwrap()).unwrap();
    assert_eq!(
        check["languages"][0]["missing"],
        json!(["menu.save", "bye"])
    );
    let one: Value =
        serde_json::from_str(&run(json!({"action": "check_language", "language": "en"})).unwrap())
            .unwrap();
    assert_eq!(one["translated"], 1);
    let added: Value =
        serde_json::from_str(&run(json!({"action": "add_language", "language": "es"})).unwrap())
            .unwrap();
    assert_eq!(added["report"]["empty"].as_array().unwrap().len(), 3);
    assert!(
        run(json!({"action": "check_language"})).is_err(),
        "a language is required"
    );
    assert!(mcp_tool("other", &json!({}), &root).is_none());
    let _ = std::fs::remove_dir_all(&root);
}
