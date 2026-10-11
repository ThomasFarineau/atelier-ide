use super::*;

fn project(name: &str, files: &[(&str, &str)]) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("atelier-run-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (path, text) in files {
        let path = dir.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn configurations_are_saved_without_the_temporary_ones() {
    let root = project("save", &[]);
    let mut configs = RunConfigs::load(&root);
    assert!(configs.configurations.is_empty(), "no file: none");
    configs.configurations.push(RunConfig {
        dir: "web".into(),
        ..RunConfig::new("serve", "npm run dev")
    });
    let temporary = configs.temporary("gradle build", "gradle build");
    assert!(temporary.temporary);
    assert_eq!(configs.selected.as_deref(), Some("gradle build"));
    configs.save(&root).unwrap();
    let read = RunConfigs::load(&root);
    assert_eq!(
        read.configurations.len(),
        1,
        "the temporary one is not saved"
    );
    assert_eq!(read.configurations[0].dir, "web");
    assert_eq!(read.selected, None, "nor the choice of it");
    configs.selected = Some("serve".into());
    configs.save(&root).unwrap();
    assert_eq!(RunConfigs::load(&root).selected.as_deref(), Some("serve"));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn temporary_configurations_are_taken_again_and_few() {
    let mut configs = RunConfigs::default();
    configs.configurations.push(RunConfig::new("build", "make"));
    let first = configs.temporary("build", "gradle build");
    assert_eq!(first.name, "build (2)", "a saved one keeps its name");
    let again = configs.temporary("build", "gradle build");
    assert_eq!(
        again.name, "build (2)",
        "the same run is the same configuration"
    );
    for i in 0..8 {
        configs.temporary(&format!("task {i}"), &format!("gradle task{i}"));
    }
    let temporary = configs
        .configurations
        .iter()
        .filter(|c| c.temporary)
        .count();
    assert_eq!(temporary, 5);
    assert!(
        configs.get("build").is_some(),
        "a saved one is never dropped"
    );
    assert!(configs.get("task 0").is_none(), "the oldest goes first");
}

#[test]
fn the_line_starts_in_the_working_directory() {
    let root = std::path::Path::new("/p");
    assert_eq!(RunConfig::new("a", "cargo run").line(root), "cargo run");
    let line = RunConfig {
        dir: "web/".into(),
        ..RunConfig::new("a", "npm start")
    }
    .line(root);
    assert!(
        line.ends_with("npm start") && line.contains("web"),
        "{line}"
    );
}

#[test]
fn the_project_suggests_its_tools() {
    let root = project(
        "suggest",
        &[
            ("Cargo.toml", "[package]\nname = \"x\"\n"),
            (
                "package.json",
                "{\"scripts\": {\"dev\": \"vite\", \"test\": \"vitest\"}}",
            ),
            ("pnpm-lock.yaml", ""),
            ("build.gradle.kts", ""),
        ],
    );
    let names: Vec<String> = suggestions(&root).into_iter().map(|c| c.name).collect();
    assert!(names.contains(&"cargo run".to_string()), "{names:?}");
    assert!(
        names.contains(&"pnpm dev".to_string()),
        "the manager of the lock file"
    );
    assert!(names.contains(&"gradle build".to_string()));
    assert!(!names.iter().any(|n| n.starts_with("mvn")), "no pom.xml");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_dialog_refuses_what_cannot_run() {
    let ok = [RunConfig::new("a", "x"), RunConfig::new("b", "y")];
    assert!(check(&ok).is_ok());
    assert!(check(&[RunConfig::new(" ", "x")]).is_err());
    assert!(check(&[RunConfig::new("a", " ")]).is_err());
    assert!(check(&[RunConfig::new("a", "x"), RunConfig::new("a", "y")]).is_err());
}
