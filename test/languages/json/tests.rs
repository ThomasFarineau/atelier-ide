use super::*;
use crate::plugins::builtins::node::PACKAGE_SCHEMA;
use parse::{Spot, Step, clean, spot};

fn provider() -> (Json, Registry) {
    let registry = Registry::default();
    registry.set(PACKAGE_SCHEMA.iter().collect());
    (Json::new(registry.clone()), registry)
}

fn doc(path: &str) -> DocRef {
    DocRef {
        key: 1,
        path: Some(PathBuf::from(path)),
        language: "json".into(),
    }
}

fn problems(json: &mut Json, path: &str, text: &str) -> Vec<(String, String)> {
    json.update(&doc(path), text, 1);
    json.diagnostics(1)
        .iter()
        .map(|d| (text[d.range.clone()].to_string(), d.message.clone()))
        .collect()
}

#[test]
fn comments_and_trailing_commas_become_spaces() {
    let text = "{\n  // a\n  \"a\": 1, /* b */\n  \"s\": \"// not a comment\",\n}";
    let cleaned = clean(text);
    assert_eq!(cleaned.len(), text.len());
    assert!(serde_json::from_str::<Value>(&cleaned).is_ok());
    assert!(cleaned.contains("\"// not a comment\""));
}

#[test]
fn the_spot_of_the_caret() {
    let text = "{\"name\": \"x\", \"scripts\": {\"build\": \"tsc\", ";
    assert_eq!(
        spot(text, text.len()),
        Spot {
            path: vec![Step::Key("scripts".into())],
            key: true,
            present: vec!["build".into()],
            in_string: None,
        }
    );
    let text = "{\"type\": \"mo";
    let at = spot(text, text.len());
    assert_eq!(at.path, vec![Step::Key("type".into())]);
    assert!(!at.key);
    assert_eq!(at.in_string, Some(10));
    let text = "{\"files\": [\"a\", ";
    assert_eq!(
        spot(text, text.len()).path,
        vec![Step::Key("files".into()), Step::Index(1)]
    );
}

#[test]
fn package_json_values_are_checked() {
    let (mut json, _) = provider();
    let text = "{\n  \"name\": \"My App\",\n  \"private\": \"yes\",\n  \"type\": \"esm\",\n  \"dependencies\": { \"react\": 18 },\n  \"anything\": 1\n}";
    let found = problems(&mut json, "web/package.json", text);
    let places: Vec<&str> = found.iter().map(|(at, _)| at.as_str()).collect();
    assert_eq!(places, ["\"My App\"", "\"yes\"", "\"esm\"", "18"]);
    assert!(found[1].1.contains("boolean"));
    assert!(found[2].1.contains("\"module\""));
}

#[test]
fn other_files_have_no_schema_and_syntax_errors_are_found() {
    let (mut json, _) = provider();
    assert!(problems(&mut json, "data.json", "{\"private\": \"yes\"}").is_empty());
    // A comment in the settings of a tool is fine.
    assert!(problems(&mut json, "tsconfig.json", "{\n  // c\n  \"a\": 1,\n}").is_empty());
    let found = problems(&mut json, "data.json", "{\n  \"a\": 1\n  \"b\": 2\n}");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].0, "\"");
}

#[test]
fn turning_the_plugin_off_drops_its_schema() {
    let (mut json, registry) = provider();
    let text = "{\"private\": \"yes\"}";
    assert_eq!(problems(&mut json, "package.json", text).len(), 1);
    registry.set(Vec::new());
    assert!(json.diagnostics(1).is_empty());
}

#[test]
fn completion_of_keys_and_values() {
    let (mut json, _) = provider();
    let text = "{\n  \"name\": \"x\",\n  ";
    let completion = json
        .complete(&doc("package.json"), text, text.len(), true)
        .take()
        .flatten()
        .unwrap();
    assert_eq!(completion.from, text.len());
    assert!(!completion.items.iter().any(|i| i.label == "name"));
    let scripts = completion
        .items
        .iter()
        .find(|i| i.label == "scripts")
        .unwrap();
    assert_eq!(scripts.insert, "\"scripts\": {}");
    assert_eq!(scripts.caret, Some("\"scripts\": {".len()));
    assert!(scripts.doc.as_deref().unwrap().contains("npm run"));
    let author = completion
        .items
        .iter()
        .find(|i| i.label == "author")
        .unwrap();
    assert!(author.doc.as_deref().unwrap().contains("wrote"));

    let text = "{\"type\": \"";
    let completion = json
        .complete(&doc("package.json"), text, text.len(), true)
        .take()
        .flatten()
        .unwrap();
    let labels: Vec<&str> = completion.items.iter().map(|i| i.insert.as_str()).collect();
    assert_eq!(labels, ["commonjs", "module"]);
    assert_eq!(completion.from, text.len());

    let text = "{\"private\": ";
    let completion = json
        .complete(&doc("package.json"), text, text.len(), true)
        .take()
        .flatten()
        .unwrap();
    let labels: Vec<&str> = completion.items.iter().map(|i| i.insert.as_str()).collect();
    assert_eq!(labels, ["true", "false"]);
}

#[test]
fn hover_describes_the_key() {
    let (mut json, _) = provider();
    let text = "{\"private\": true}";
    let hover = json
        .hover(&doc("package.json"), text, 3)
        .take()
        .flatten()
        .unwrap();
    assert_eq!(hover.title, "\"private\": boolean");
    assert!(hover.body.contains("publishing"));
    assert!(hover.body.contains("package.json"));
}

#[test]
fn a_file_names_its_schema() {
    let (mut json, _) = provider();
    let text = "{\"$schema\": \"https://json.schemastore.org/package.json\", \"private\": 1}";
    assert_eq!(problems(&mut json, "other.json", text).len(), 1);
    let dir = std::env::temp_dir().join(format!("atelier-json-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("schemas")).unwrap();
    std::fs::write(
        dir.join("schemas/app.json"),
        r#"{"type": "object", "required": ["port"], "additionalProperties": false,
            "properties": {"port": {"type": "integer", "minimum": 1}}}"#,
    )
    .unwrap();
    let text = "{\"$schema\": \"./schemas/app.json\", \"host\": \"x\", \"port\": 0}";
    let path = dir.join("app.config.json");
    let found = problems(&mut json, path.to_str().unwrap(), text);
    let places: Vec<&str> = found.iter().map(|(at, _)| at.as_str()).collect();
    assert_eq!(places, ["\"host\"", "0"]);
    let found = problems(
        &mut json,
        path.to_str().unwrap(),
        "{\"$schema\": \"./schemas/app.json\"}",
    );
    assert_eq!(found[0].1, trf!("json.missing_properties", keys = "port"));
    // An address no plugin brings is not downloaded.
    let text = "{\"$schema\": \"https://example.com/s.json\", \"a\": 1}";
    assert!(problems(&mut json, "x.json", text).is_empty());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn json_is_still_reformatted() {
    let (mut json, _) = provider();
    let edits = json
        .format(&doc("a.json"), "{\"a\":1}")
        .take()
        .unwrap()
        .unwrap();
    assert_eq!(format::apply("{\"a\":1}", &edits), "{\n  \"a\": 1\n}");
}

#[test]
fn schemas_apply_to_names_and_ends_of_paths() {
    let schema = Schema {
        name: "w",
        files: &["package.json", ".github/workflows/*.yml"],
        urls: &[],
        text: "{}",
    };
    assert!(schema.applies_to(Path::new("a/package.json")));
    assert!(schema.applies_to(Path::new("repo/.github/workflows/ci.yml")));
    assert!(!schema.applies_to(Path::new("repo/.github/ci.yml")));
    assert!(!schema.applies_to(Path::new("repo/.github/workflows/ci.yaml")));
    assert!(!schema.applies_to(Path::new("workflows/ci.yml")));
}

fn yaml_doc(path: &str) -> DocRef {
    DocRef {
        key: 2,
        path: Some(PathBuf::from(path)),
        language: "yaml".into(),
    }
}

#[test]
fn yaml_documents_keep_the_places_of_their_values() {
    let text =
        "name: app\nlist:\n  - 'a b'\n  - 3\nbase: &b\n  x: 1\nchild:\n  <<: *b\n  y: true\n";
    let documents = yaml::parse(text).ok().unwrap();
    let document = &documents[0];
    assert_eq!(
        document.value(),
        serde_json::json!({"name": "app", "list": ["a b", 3], "base": {"x": 1}, "child": {"y": true, "x": 1}})
    );
    let name = document.member("name").unwrap();
    assert_eq!(&text[name.key_range.clone()], "name");
    assert_eq!(&text[name.value.range.clone()], "app");
    let list = &document.member("list").unwrap().value;
    let parse::Kind::Array(items) = &list.kind else {
        panic!("a list")
    };
    assert_eq!(&text[items[0].range.clone()], "'a b'");
    assert!(yaml::parse("a: [1, 2\nb: 3\n").is_err());
}

#[test]
fn yaml_spots_come_from_the_documents() {
    let text = "jobs:\n  build:\n    runs-on: x\n    st";
    let (spot, from) = yaml::spot(text, text.len()).unwrap();
    assert_eq!(
        spot.path,
        vec![Step::Key("jobs".into()), Step::Key("build".into())]
    );
    assert!(spot.key);
    assert_eq!(spot.present, vec!["runs-on".to_string()]);
    assert_eq!(from, text.len() - 2);
    let text = "jobs:\n  build:\n    steps:\n      - uses: x\n        shell: b";
    let (spot, from) = yaml::spot(text, text.len()).unwrap();
    assert_eq!(
        spot.path,
        vec![
            Step::Key("jobs".into()),
            Step::Key("build".into()),
            Step::Key("steps".into()),
            Step::Index(0),
            Step::Key("shell".into())
        ]
    );
    assert!(!spot.key);
    assert_eq!(from, text.len() - 1);
}

#[test]
fn yaml_completion_hover_and_named_schemas() {
    let (mut json, _) = provider();
    let dir = std::env::temp_dir().join(format!("atelier-yaml-schema-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("s.json"),
        r#"{"type": "object", "properties": {
            "mode": {"enum": ["fast", "safe"], "description": "How it runs."},
            "server": {"type": "object", "properties": {"port": {"type": "integer"}}}}}"#,
    )
    .unwrap();
    let path = dir.join("c.yaml");
    let path = path.to_str().unwrap();
    let head = "# yaml-language-server: $schema=./s.json\n";
    let complete = |json: &mut Json, text: &str| {
        json.complete(&yaml_doc(path), text, text.len(), true)
            .take()
            .flatten()
            .unwrap()
    };
    let completion = complete(&mut json, &format!("{head}mo"));
    let mode = completion.items.iter().find(|i| i.label == "mode").unwrap();
    assert_eq!(mode.insert, "mode: ");
    let server = completion
        .items
        .iter()
        .find(|i| i.label == "server")
        .unwrap();
    assert_eq!(server.insert, "server:\n  ");
    let completion = complete(&mut json, &format!("{head}mode: "));
    let values: Vec<&str> = completion.items.iter().map(|i| i.insert.as_str()).collect();
    assert_eq!(values, ["fast", "safe"]);
    let text = format!("{head}mode: slow\nserver:\n  port: x\n");
    json.update(&yaml_doc(path), &text, 1);
    let found: Vec<String> = json
        .diagnostics(2)
        .iter()
        .map(|d| text[d.range.clone()].to_string())
        .collect();
    assert_eq!(found, ["slow", "x"]);
    let at = text.find("mode").unwrap() + 1;
    let hover = json
        .hover(&yaml_doc(path), &text, at)
        .take()
        .flatten()
        .unwrap();
    assert!(hover.body.contains("How it runs."));
    // Without a schema, a YAML file is left alone: it may be a template.
    json.update(&yaml_doc("templates/x.yaml"), "a: {{ .Values.x }}\n", 2);
    assert!(json.diagnostics(2).is_empty());
    std::fs::remove_dir_all(&dir).unwrap();
}
