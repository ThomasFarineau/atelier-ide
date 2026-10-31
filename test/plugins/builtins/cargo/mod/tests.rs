use super::*;

fn project(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("atelier-cargo-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (path, text) in files {
        let path = dir.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    dir
}

fn names(package: &Package) -> Vec<(Kind, &str)> {
    package
        .targets
        .iter()
        .map(|t| (t.kind, t.name.as_str()))
        .collect()
}

#[test]
fn a_workspace_and_the_targets_of_its_members() {
    let root = project(
        "workspace",
        &[
            ("Cargo.toml", "[workspace]\nmembers = [\"crates/*\"]\n"),
            (
                "crates/app/Cargo.toml",
                "[package]\nname = \"app\"\nversion = \"0.2.0\"\n\n[[bin]]\nname = \"tool\"\npath = \"src/tool.rs\"\n",
            ),
            ("crates/app/src/main.rs", "fn main() {}"),
            ("crates/app/src/tool.rs", "fn main() {}"),
            ("crates/app/src/bin/extra.rs", "fn main() {}"),
            ("crates/app/src/bin/multi/main.rs", "fn main() {}"),
            ("crates/app/examples/demo.rs", "fn main() {}"),
            ("crates/app/tests/it.rs", ""),
            ("crates/app/benches/speed.rs", ""),
            (
                "crates/lib/Cargo.toml",
                "[package]\nname = \"lib\"\nautotests = false\n",
            ),
            ("crates/lib/src/lib.rs", ""),
            ("crates/lib/tests/hidden.rs", ""),
            ("target/debug/Cargo.toml", "[package]\nname = \"skipped\"\n"),
        ],
    );
    let packages = scan(&root);
    let rels: Vec<&str> = packages.iter().map(|p| p.rel.as_str()).collect();
    assert_eq!(
        rels,
        [
            "Cargo.toml",
            "crates/app/Cargo.toml",
            "crates/lib/Cargo.toml"
        ],
        "target is skipped"
    );
    assert!(packages[0].workspace && packages[0].targets.is_empty());
    let app = &packages[1];
    assert_eq!(
        (app.name.as_str(), app.version.as_deref()),
        ("app", Some("0.2.0"))
    );
    assert_eq!(
        names(app),
        [
            (Kind::Bin, "tool"),
            (Kind::Bin, "app"),
            (Kind::Bin, "extra"),
            (Kind::Bin, "multi"),
            (Kind::Example, "demo"),
            (Kind::Test, "it"),
            (Kind::Bench, "speed"),
        ],
        "the declared targets first, then the found ones"
    );
    assert_eq!(app.targets[0].path, Some(app.dir.join("src/tool.rs")));
    assert!(names(&packages[2]).is_empty(), "autotests = false");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_target_runs_with_its_flag() {
    let target = |kind, name: &str| Target {
        kind,
        name: name.into(),
        path: None,
    };
    assert_eq!(target(Kind::Bin, "app").args(), ["run", "--bin", "app"]);
    assert_eq!(
        target(Kind::Example, "demo").args(),
        ["run", "--example", "demo"]
    );
    assert_eq!(target(Kind::Test, "it").args(), ["test", "--test", "it"]);
    assert_eq!(
        target(Kind::Bench, "speed").args(),
        ["bench", "--bench", "speed"]
    );
}

#[test]
fn the_plugin_applies_only_where_there_is_a_cargo_toml() {
    let dir = project("none", &[]);
    let mut plugin = CargoPlugin {
        packages: scan(&dir),
        ..Default::default()
    };
    assert!(!plugin.applies());
    std::fs::write(dir.join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
    plugin.packages = scan(&dir);
    assert!(plugin.applies());
    let _ = std::fs::remove_dir_all(&dir);
}
