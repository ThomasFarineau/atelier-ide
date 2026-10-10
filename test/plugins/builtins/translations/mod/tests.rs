use super::*;
use catalog::{Format, discover, is_locale, placeholders};

fn file(path: PathBuf) -> Source {
    Source::File(path)
}

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
    let stem = discover(&file(root.join("locales/fr_FR.json"))).unwrap();
    assert_eq!(stem.source.language, "fr_FR");
    assert_eq!(
        stem.others
            .iter()
            .map(|f| f.language.as_str())
            .collect::<Vec<_>>(),
        ["en_US"],
        "settings.json is no language"
    );
    assert!(!stem.namespaced);
    assert_eq!(stem.path_for("es"), root.join("locales/es.json"));
    let folder = discover(&file(root.join("i18n/fr/common.json"))).unwrap();
    assert_eq!(folder.others[0].path(), root.join("i18n/de/common.json"));
    assert_eq!(folder.path_for("es"), root.join("i18n/es/common.json"));
    let bundle = discover(&file(root.join("res/messages.properties"))).unwrap();
    assert_eq!(bundle.source.parts[0].format, Format::Properties);
    assert_eq!(bundle.source.language, "default");
    assert_eq!(bundle.others[0].language, "it");
    assert_eq!(
        bundle.path_for("es"),
        root.join("res/messages_es.properties")
    );
    assert!(
        discover(&file(root.join("locales/settings.json"))).is_err(),
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
    let catalog = discover(&file(root.join("locales/fr.json"))).unwrap();
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
    let catalog = discover(&file(root.join("locales/fr.json"))).unwrap();
    let path = catalog::add_language(&catalog, "de", Fill::Empty)
        .unwrap()
        .remove(0);
    let value: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(value["menu"]["save"], "", "same structure, empty texts");
    let catalog = discover(&file(root.join("locales/fr.json"))).unwrap();
    assert!(
        catalog::add_language(&catalog, "de", Fill::Empty).is_err(),
        "already there"
    );
    assert!(
        catalog::add_language(&catalog, "../x", Fill::Empty).is_err(),
        "not a language"
    );
    let path = catalog::add_language(&catalog, "it", Fill::Source)
        .unwrap()
        .remove(0);
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

#[test]
fn a_language_folder_is_a_source_of_namespaces() {
    let root = project(
        "folder",
        &[
            ("locales/fr/common.json", FR),
            ("locales/fr/auth/login.json", "{\"title\": \"Connexion\"}"),
            ("locales/en/common.json", "{\"menu\": {\"open\": \"Open\"}}"),
            ("locales/readme.json", "{}"),
        ],
    );
    let source = Source::from_path(&root.join("locales/fr"));
    assert!(matches!(source, Source::Folder(_)));
    let catalog = discover(&source).unwrap();
    assert!(catalog.namespaced);
    assert_eq!(catalog.source.language, "fr");
    assert_eq!(catalog.source.parts.len(), 2);
    assert_eq!(catalog.others[0].language, "en");
    let keys: Vec<String> = catalog
        .read(&catalog.source)
        .unwrap()
        .into_iter()
        .map(|(k, _)| k)
        .collect();
    assert!(keys.contains(&"auth/login:title".to_string()), "{keys:?}");
    assert!(keys.contains(&"common:menu.open".to_string()));
    let report = catalog::check(&catalog, "en").unwrap();
    assert_eq!(report.total, 4);
    assert_eq!(report.translated, 1);
    // A key of a namespace English does not have yet creates its file.
    catalog.set("en", "auth/login:title", "Sign in").unwrap();
    let written = std::fs::read_to_string(root.join("locales/en/auth/login.json")).unwrap();
    assert!(written.contains("Sign in"), "{written}");
    assert!(
        catalog.set("en", "title", "x").is_err(),
        "a namespace is needed"
    );
    let created = catalog::add_language(&catalog, "de", Fill::Empty).unwrap();
    assert_eq!(created.len(), 2);
    assert!(root.join("locales/de/auth/login.json").is_file());
    let catalog = discover(&source).unwrap();
    assert!(catalog.folders().contains(&root.join("locales")));
    catalog.remove("common:bye").unwrap();
    assert!(
        !std::fs::read_to_string(root.join("locales/fr/common.json"))
            .unwrap()
            .contains("bye")
    );
    assert!(
        discover(&Source::Folder(root.join("locales"))).is_err(),
        "not the folder of a language"
    );
    let candidates = catalog::candidates(&root);
    assert!(
        candidates.contains(&root.join("locales/fr")),
        "{candidates:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_pattern_finds_languages_and_namespaces_anywhere() {
    let root = project(
        "pattern",
        &[
            ("app/shop/lang/en.json", "{\"buy\": \"Buy\"}"),
            ("app/shop/lang/fr.json", "{\"buy\": \"Acheter\"}"),
            ("app/cart/lang/en.json", "{\"empty\": \"Empty cart\"}"),
            ("app/cart/lang/package.json", "{}"),
            ("node_modules/x/lang/en.json", "{}"),
        ],
    );
    let pattern = |text: &str, language: &str| Source::Pattern {
        root: root.clone(),
        pattern: text.into(),
        language: language.into(),
    };
    let catalog = discover(&pattern(r"app/(?<ns>\w+)/lang/(?<lang>[a-z]{2})\.json", "")).unwrap();
    assert_eq!(catalog.source.language, "en", "English by default");
    assert_eq!(catalog.source.parts.len(), 2, "node_modules is skipped");
    assert!(catalog.namespaced);
    let report = catalog::check(&catalog, "fr").unwrap();
    assert_eq!(
        report.problems,
        [("cart:empty".to_string(), Problem::Missing)]
    );
    catalog.set("fr", "cart:empty", "Panier vide").unwrap();
    assert!(root.join("app/cart/lang/fr.json").is_file());
    // Without a namespace group, the one file of each language: plain keys.
    let one = discover(&pattern(r"app/shop/lang/(?<lang>\w+)\.json", "fr")).unwrap();
    assert!(!one.namespaced);
    assert_eq!(one.source.language, "fr");
    assert_eq!(one.path_for("de"), root.join("app/shop/lang/de.json"));
    assert!(
        discover(&pattern(r"app/.*\.json", "")).is_err(),
        "no lang group"
    );
    assert!(discover(&pattern(r"nothing/(?<lang>\w+)\.json", "")).is_err());
    assert!(
        discover(&pattern(r"app/(?<lang>[a-z]{2}", "")).is_err(),
        "invalid"
    );
    // The choice is kept.
    let chosen = pattern(r"app/shop/lang/(?<lang>\w+)\.json", "fr");
    catalog::set_source(&root, &chosen).unwrap();
    assert_eq!(catalog::source_of(&root), Some(chosen));
    catalog::set_source(&root, &Source::File(root.join("app/shop/lang/en.json"))).unwrap();
    assert_eq!(
        catalog::source_of(&root),
        Some(Source::File(root.join("app/shop/lang/en.json"))),
        "the pattern is replaced"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_mcp_tool_takes_a_pattern() {
    let root = project(
        "mcp-pattern",
        &[
            ("i18n/messages.en.json", "{\"hi\": \"Hello\"}"),
            ("i18n/messages.fr.json", "{}"),
        ],
    );
    let run = |args: Value| mcp_tool("i18n", &args, &root).unwrap();
    let check: Value = serde_json::from_str(
        &run(json!({"action": "check", "pattern": r"i18n/messages\.(?<lang>\w+)\.json"})).unwrap(),
    )
    .unwrap();
    assert_eq!(check["source"], "en");
    assert_eq!(check["languages"][0]["missing"], json!(["hi"]));
    let _ = std::fs::remove_dir_all(&root);
}
