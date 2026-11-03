use super::*;

#[test]
fn convert_to_json_is_offered_on_yaml_files_only() {
    let plugin = YamlPlugin;
    assert_eq!(plugin.file_commands(Path::new("ci.yml")).len(), 1);
    assert_eq!(plugin.file_commands(Path::new("k8s/Deploy.YAML")).len(), 1);
    assert!(plugin.file_commands(Path::new("Cargo.toml")).is_empty());
    assert!(plugin.file_commands(Path::new("yml")).is_empty());
}

#[test]
fn convert_to_json_writes_next_to_the_file_and_never_over_one() {
    let dir = std::env::temp_dir().join(format!("atelier-yaml-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let source = dir.join("config.yml");
    std::fs::write(&source, "\u{feff}a: 1\n").unwrap();
    let target = convert_to_json(&source).unwrap();
    assert_eq!(target, dir.join("config.json"));
    assert_eq!(
        std::fs::read_to_string(&target).unwrap(),
        "{\n  \"a\": 1\n}\n"
    );
    assert!(convert_to_json(&source).is_err());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn colors_and_formats_yaml_and_toml() {
    let plugin = YamlPlugin;
    assert_eq!(plugin.languages(), ["yaml", "toml"]);
    let languages: Vec<_> = plugin.formatters().iter().map(|(l, _)| *l).collect();
    assert_eq!(languages, ["yaml", "toml"]);
}
